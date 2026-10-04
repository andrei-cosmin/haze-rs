//! The record `#[haze::resource]` submits, and the resolver that runs them.

use std::{
    any::TypeId,
    fmt::{self, Debug, Formatter},
    pin::Pin,
    ptr,
};

use anyhow::{Context, Error, Result, anyhow, bail};

use crate::{Resources, inject::need::Need};

/// Runs one resource function and inserts what it returns.
type Provide = for<'a> fn(&'a mut Resources) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>>;

/// One function recorded by `#[haze::resource]`: its name, the type it
/// provides, the resource types it needs or takes optionally, and how to run it.
#[doc(hidden)]
pub struct Provider {
    /// The function's full path, for error messages and startup order.
    name: &'static str,
    /// The type it inserts.
    provides: Need,
    /// The types of its required parameters, which it waits for.
    needs: &'static [Need],
    /// The `Option<T>` types it takes as `None` when nobody provides them.
    optional: &'static [Need],
    /// Runs the function and inserts what it returns.
    provide: Provide,
}

inventory::collect!(Provider);

impl Provider {
    /// Describes one resource function.
    #[must_use]
    pub const fn new(
        name: &'static str,
        provides: Need,
        needs: &'static [Need],
        optional: &'static [Need],
        provide: Provide,
    ) -> Self {
        Self {
            name,
            provides,
            needs,
            optional,
            provide,
        }
    }

    /// Every registered resource function, sorted by name so startup runs in
    /// the same order on every build.
    fn all() -> Vec<&'static Self> {
        let mut providers = Vec::new();
        for provider in inventory::iter::<Self> {
            providers.push(provider);
        }
        providers.sort_by_key(|provider| provider.name);
        providers
    }

    /// The name of the provided type, for error messages.
    fn provided_name(&self) -> &'static str {
        self.provides.name()
    }

    /// The [`TypeId`] of the provided type.
    fn provided_id(&self) -> TypeId {
        self.provides.id()
    }

    /// Runs every registered resource function whose type is not in `resources` yet.
    pub(crate) async fn resolve(resources: &mut Resources) -> Result<()> {
        Self::resolve_all(resources, Self::all()).await
    }

    /// Runs the given functions, so tests can pass their own instead of the
    /// registered ones. The error after the fallback is defensive: once
    /// `reject_unsatisfiable` passed, a round without progress always leaves a
    /// cycle whose first function can run, unless a hand-written provider
    /// returned without inserting its type.
    async fn resolve_all(resources: &mut Resources, providers: Vec<&Self>) -> Result<()> {
        Self::reject_duplicates(&providers)?;
        let mut pending = Vec::new();
        for provider in providers {
            if !resources.contains_type_id(provider.provided_id()) {
                pending.push(provider);
            }
        }
        Self::reject_unsatisfiable(resources, &pending)?;

        while !pending.is_empty() {
            let mut waiting = Vec::new();
            for provider in &pending {
                waiting.push(provider.provided_id());
            }
            let mut progress = false;
            let mut next_round = Vec::new();
            for provider in pending {
                if provider.can_run(resources, &waiting) {
                    provider.run(resources).await?;
                    progress = true;
                } else {
                    next_round.push(provider);
                }
            }
            if !progress {
                let mut unblocked = None;
                for (index, provider) in next_round.iter().enumerate() {
                    if provider.can_run(resources, &[])
                        && provider.waits_only_on_its_cycle(&next_round)
                    {
                        unblocked = Some(index);
                        break;
                    }
                }
                let Some(index) = unblocked else {
                    return Err(Self::stuck(resources, &[], &next_round));
                };
                let provider = next_round.remove(index);
                provider.run(resources).await?;
            }
            pending = next_round;
        }
        Ok(())
    }

    /// Runs this function, naming it and its type if it fails.
    async fn run(&self, resources: &mut Resources) -> Result<()> {
        (self.provide)(resources)
            .await
            .with_context(|| format!("{} failed to provide {}", self.name, self.provided_name()))
    }

    /// Whether every required type exists and no optional type is still coming.
    fn can_run(&self, resources: &Resources, waiting: &[TypeId]) -> bool {
        for need in self.needs {
            if !resources.contains_type_id(need.id()) {
                return false;
            }
        }
        for need in self.optional {
            if !resources.contains_type_id(need.id()) && waiting.contains(&need.id()) {
                return false;
            }
        }
        true
    }

    /// Whether this function takes what `other` provides as a required parameter.
    fn requires(&self, other: &Self) -> bool {
        for need in self.needs {
            if need.id() == other.provided_id() {
                return true;
            }
        }
        false
    }

    /// Fails before anything runs when some function can never get its required
    /// types, simulating the run on types alone with optionals treated as met.
    fn reject_unsatisfiable(resources: &Resources, providers: &[&Self]) -> Result<()> {
        let mut produced = Vec::new();
        let mut remaining = providers.to_vec();
        loop {
            let mut next = Vec::new();
            for provider in &remaining {
                let mut ready = true;
                for need in provider.needs {
                    if !resources.contains_type_id(need.id()) && !produced.contains(&need.id()) {
                        ready = false;
                        break;
                    }
                }
                if ready {
                    produced.push(provider.provided_id());
                } else {
                    next.push(*provider);
                }
            }
            if next.len() == remaining.len() {
                break;
            }
            remaining = next;
        }
        if remaining.is_empty() {
            return Ok(());
        }
        Err(Self::stuck(resources, &produced, &remaining))
    }

    /// Whether every function this one waits on, directly or through others,
    /// leads back to it, so running it with `None` gives up only on types of
    /// the cycle it is part of, and only once no cycle it waits on is left.
    fn waits_only_on_its_cycle(&self, providers: &[&Self]) -> bool {
        for other in providers {
            if self.path_to(other, providers, Self::waits_on).is_none() {
                continue;
            }
            if other.path_to(self, providers, Self::waits_on).is_none() {
                return false;
            }
        }
        true
    }

    /// The functions from `self` to `target`, each leading to the next by
    /// `leads_to`, or `None` when `target` is never reached.
    fn path_to<'a>(
        &'a self,
        target: &Self,
        providers: &[&'a Self],
        leads_to: impl Fn(&Self, &Self) -> bool,
    ) -> Option<Vec<&'a Self>> {
        let mut path = vec![self];
        let mut visited = vec![self];
        let mut stack = vec![(self, providers.iter())];
        while let Some((current, others)) = stack.last_mut() {
            let current = *current;
            let Some(&next) = others.next() else {
                path.pop();
                stack.pop();
                continue;
            };
            if !leads_to(current, next) {
                continue;
            }
            if ptr::eq(next, target) {
                return Some(path);
            }
            let mut seen = false;
            for done in &visited {
                if ptr::eq(*done, next) {
                    seen = true;
                    break;
                }
            }
            if seen {
                continue;
            }
            visited.push(next);
            path.push(next);
            stack.push((next, providers.iter()));
        }
        None
    }

    /// Whether this function takes the type `other` provides, as `T` or
    /// `Option<T>`.
    fn waits_on(&self, other: &Self) -> bool {
        for need in self.needs.iter().chain(self.optional) {
            if need.id() == other.provided_id() {
                return true;
            }
        }
        false
    }

    /// Fails when two functions provide the same type, naming all of them.
    fn reject_duplicates(providers: &[&Self]) -> Result<()> {
        let mut duplicates = Vec::new();
        for (index, first) in providers.iter().enumerate() {
            let mut names = Vec::new();
            for (earlier, other) in providers.iter().enumerate() {
                if other.provided_id() != first.provided_id() {
                    continue;
                }
                if earlier < index {
                    names.clear();
                    break;
                }
                names.push(other.name);
            }
            if names.len() > 1 {
                duplicates.push(format!(
                    "{} is provided by more than one function: {}",
                    first.provided_name(),
                    names.join(", ")
                ));
            }
        }
        if duplicates.is_empty() {
            return Ok(());
        }
        duplicates.sort();
        bail!("{}", duplicates.join("; "))
    }

    /// Explains why each function cannot run, naming each required cycle once;
    /// `extra` holds types that will exist but are not inserted yet.
    fn stuck(resources: &Resources, extra: &[TypeId], providers: &[&Self]) -> Error {
        let mut problems = Vec::new();
        for provider in providers {
            for need in provider.needs {
                if resources.contains_type_id(need.id()) || extra.contains(&need.id()) {
                    continue;
                }
                let mut maker = None;
                for other in providers {
                    if other.provided_id() == need.id() {
                        maker = Some(other);
                        break;
                    }
                }
                let (reason, ring) = match maker {
                    Some(other) => match other.path_to(provider, providers, Self::requires) {
                        Some(cycle) => (
                            Self::cycle_reason(provider, &cycle),
                            Some(Self::ring(provider, &cycle)),
                        ),
                        None => (
                            format!(
                                "which only {0} provides, and {0} cannot run either",
                                other.name
                            ),
                            None,
                        ),
                    },
                    None => (
                        "which was never inserted, and no #[haze::resource] provides it".to_owned(),
                        None,
                    ),
                };
                problems.push((
                    format!("{} needs {}, {reason}", provider.name, need.name()),
                    ring,
                ));
            }
        }
        problems.sort();
        let mut rings = Vec::new();
        let mut lines = Vec::new();
        for (line, ring) in problems {
            if let Some(ring) = ring {
                if rings.contains(&ring) {
                    continue;
                }
                rings.push(ring);
            }
            lines.push(line);
        }
        lines.dedup();
        anyhow!("resource functions cannot run: {}", lines.join("; "))
    }

    /// The names on the required cycle from `provider` through `cycle`, starting
    /// from the first by name, so the same cycle found from another member
    /// compares equal and is reported once.
    fn ring(provider: &Self, cycle: &[&Self]) -> Vec<&'static str> {
        let mut names = vec![provider.name];
        for other in cycle {
            names.push(other.name);
        }
        let mut first = 0;
        for (index, name) in names.iter().enumerate() {
            if *name < names[first] {
                first = index;
            }
        }
        names.rotate_left(first);
        names
    }

    /// Names each function on a required cycle in turn, from the first of
    /// `cycle` back to `provider`, the way bevy names every member of a
    /// schedule cycle.
    fn cycle_reason(provider: &Self, cycle: &[&Self]) -> String {
        let mut clauses = Vec::with_capacity(cycle.len());
        for (index, other) in cycle.iter().enumerate() {
            let needed = match cycle.get(index + 1) {
                Some(next) => next.provided_name().to_owned(),
                None => format!("{} back", provider.provided_name()),
            };
            clauses.push(format!(
                "which only {0} provides, and {0} needs {needed}",
                other.name
            ));
        }
        format!(
            "{}; take one of them as Later<T> to break the cycle",
            clauses.join(", ")
        )
    }
}

impl Debug for Provider {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Provider")
            .field("name", &self.name)
            .field("provides", &self.provides)
            .field("needs", &self.needs)
            .field("optional", &self.optional)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;

    use anyhow::{Result, anyhow};

    use super::Provider;
    use crate::{Resources, inject::need::Need};

    type Provided<'a> = Pin<Box<dyn Future<Output = Result<()>> + 'a>>;

    struct Fixtures;

    impl Fixtures {
        fn number(resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move {
                resources.insert(7_u64);
                Ok(())
            })
        }

        fn text(resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move {
                let number = resources.try_get::<u64>()?;
                resources.insert(format!("number {number}"));
                Ok(())
            })
        }

        fn greeting(resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move {
                let number = resources.get::<u64>();
                resources.insert(format!("{number:?}"));
                Ok(())
            })
        }

        fn ping(resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move {
                let pong = resources.get::<u16>();
                resources.insert(if pong.is_some() { 2_u8 } else { 1_u8 });
                Ok(())
            })
        }

        fn pong(resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move {
                let ping = resources.get::<u8>();
                resources.insert(if ping.is_some() { 20_u16 } else { 10_u16 });
                Ok(())
            })
        }

        fn failing(_resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move { Err(anyhow!("disk is full")) })
        }

        fn unreachable(_resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move { Err(anyhow!("must not run")) })
        }

        fn marker(resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move {
                resources.insert(1_i32);
                Ok(())
            })
        }

        fn needs_back(resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move {
                let back = resources.try_get::<f64>()?;
                resources.insert(format!("back {back}").len());
                Ok(())
            })
        }

        fn gives_back(resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move {
                let front = resources.get::<usize>();
                resources.insert(if front.is_some() { 2.0_f64 } else { 1.0_f64 });
                Ok(())
            })
        }

        fn outsider(resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move {
                let pong = resources.get::<u16>();
                resources.insert(i128::from(pong.is_some()));
                Ok(())
            })
        }

        fn bridge(resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move {
                let pong = resources.get::<u16>();
                let echo = resources.get::<&'static str>();
                resources.insert(10 * isize::from(pong.is_some()) + isize::from(echo.is_some()));
                Ok(())
            })
        }

        fn echo(resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move {
                let bridge = resources.get::<isize>();
                resources.insert(if bridge.is_some() { "after" } else { "before" });
                Ok(())
            })
        }

        fn head(resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move {
                let tail = resources.get::<Vec<u32>>();
                resources.insert(if tail.is_some() {
                    vec![2_u8]
                } else {
                    vec![1_u8]
                });
                Ok(())
            })
        }

        fn body(resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move {
                let head = resources.get::<Vec<u8>>();
                resources.insert(if head.is_some() {
                    vec![2_u16]
                } else {
                    vec![1_u16]
                });
                Ok(())
            })
        }

        fn tail(resources: &mut Resources) -> Provided<'_> {
            Box::pin(async move {
                let body = resources.get::<Vec<u16>>();
                resources.insert(if body.is_some() {
                    vec![2_u32]
                } else {
                    vec![1_u32]
                });
                Ok(())
            })
        }
    }

    const NUMBER: Provider = Provider::new(
        "tests::number",
        Need::of::<u64>(),
        &[],
        &[],
        Fixtures::number,
    );

    const OTHER_NUMBER: Provider = Provider::new(
        "tests::other_number",
        Need::of::<u64>(),
        &[],
        &[],
        Fixtures::number,
    );

    const TEXT: Provider = Provider::new(
        "tests::text",
        Need::of::<String>(),
        &[Need::of::<u64>()],
        &[],
        Fixtures::text,
    );

    const GREETING: Provider = Provider::new(
        "tests::greeting",
        Need::of::<String>(),
        &[],
        &[Need::of::<u64>()],
        Fixtures::greeting,
    );

    const PING: Provider = Provider::new(
        "tests::ping",
        Need::of::<u8>(),
        &[],
        &[Need::of::<u16>()],
        Fixtures::ping,
    );

    const PONG: Provider = Provider::new(
        "tests::pong",
        Need::of::<u16>(),
        &[],
        &[Need::of::<u8>()],
        Fixtures::pong,
    );

    const FAILING: Provider = Provider::new(
        "tests::failing",
        Need::of::<u32>(),
        &[],
        &[],
        Fixtures::failing,
    );

    const FIRST_OF_CYCLE: Provider = Provider::new(
        "tests::first",
        Need::of::<i8>(),
        &[Need::of::<i16>()],
        &[],
        Fixtures::unreachable,
    );

    const SECOND_OF_CYCLE: Provider = Provider::new(
        "tests::second",
        Need::of::<i16>(),
        &[Need::of::<i8>()],
        &[],
        Fixtures::unreachable,
    );

    const WAITS_ON_BLOCKED: Provider = Provider::new(
        "tests::waits_on_blocked",
        Need::of::<i32>(),
        &[],
        &[Need::of::<i64>()],
        Fixtures::marker,
    );

    const BLOCKED: Provider = Provider::new(
        "tests::blocked",
        Need::of::<i64>(),
        &[Need::of::<u128>()],
        &[],
        Fixtures::unreachable,
    );

    const OUTSIDER: Provider = Provider::new(
        "tests::a_outsider",
        Need::of::<i128>(),
        &[],
        &[Need::of::<u16>()],
        Fixtures::outsider,
    );

    const NEEDS_BACK: Provider = Provider::new(
        "tests::needs_back",
        Need::of::<usize>(),
        &[Need::of::<f64>()],
        &[],
        Fixtures::needs_back,
    );

    const GIVES_BACK: Provider = Provider::new(
        "tests::gives_back",
        Need::of::<f64>(),
        &[],
        &[Need::of::<usize>()],
        Fixtures::gives_back,
    );

    const WAITS_ON_CYCLE: Provider = Provider::new(
        "tests::a_waits_on_cycle",
        Need::of::<i32>(),
        &[],
        &[Need::of::<i8>()],
        Fixtures::marker,
    );

    const START_OF_RING: Provider = Provider::new(
        "tests::start",
        Need::of::<char>(),
        &[Need::of::<bool>()],
        &[],
        Fixtures::unreachable,
    );

    const MIDDLE_OF_RING: Provider = Provider::new(
        "tests::middle",
        Need::of::<bool>(),
        &[Need::of::<f32>()],
        &[],
        Fixtures::unreachable,
    );

    const END_OF_RING: Provider = Provider::new(
        "tests::end",
        Need::of::<f32>(),
        &[Need::of::<char>()],
        &[],
        Fixtures::unreachable,
    );

    const BRIDGE: Provider = Provider::new(
        "tests::a_bridge",
        Need::of::<isize>(),
        &[],
        &[Need::of::<&'static str>(), Need::of::<u16>()],
        Fixtures::bridge,
    );

    const ECHO: Provider = Provider::new(
        "tests::echo",
        Need::of::<&'static str>(),
        &[],
        &[Need::of::<isize>()],
        Fixtures::echo,
    );

    const HEAD_OF_RING: Provider = Provider::new(
        "tests::head",
        Need::of::<Vec<u8>>(),
        &[],
        &[Need::of::<Vec<u32>>()],
        Fixtures::head,
    );

    const BODY_OF_RING: Provider = Provider::new(
        "tests::body",
        Need::of::<Vec<u16>>(),
        &[],
        &[Need::of::<Vec<u8>>()],
        Fixtures::body,
    );

    const TAIL_OF_RING: Provider = Provider::new(
        "tests::tail",
        Need::of::<Vec<u32>>(),
        &[],
        &[Need::of::<Vec<u16>>()],
        Fixtures::tail,
    );

    const AFTER_BLOCKED: Provider = Provider::new(
        "tests::after_blocked",
        Need::of::<Vec<i64>>(),
        &[Need::of::<i64>()],
        &[],
        Fixtures::unreachable,
    );

    const NEEDS_NUMBER_AND_MISSING: Provider = Provider::new(
        "tests::needs_number_and_missing",
        Need::of::<Vec<u64>>(),
        &[Need::of::<u64>(), Need::of::<u128>()],
        &[],
        Fixtures::unreachable,
    );

    const KNOT_A: Provider = Provider::new(
        "tests::knot_a",
        Need::of::<Vec<i8>>(),
        &[Need::of::<Vec<i16>>()],
        &[],
        Fixtures::unreachable,
    );

    const KNOT_B: Provider = Provider::new(
        "tests::knot_b",
        Need::of::<Vec<i16>>(),
        &[Need::of::<Vec<i8>>(), Need::of::<Vec<i32>>()],
        &[],
        Fixtures::unreachable,
    );

    const KNOT_C: Provider = Provider::new(
        "tests::knot_c",
        Need::of::<Vec<i32>>(),
        &[Need::of::<Vec<i8>>()],
        &[],
        Fixtures::unreachable,
    );

    const TWICE: Provider = Provider::new(
        "tests::twice",
        Need::of::<Vec<u128>>(),
        &[Need::of::<u128>(), Need::of::<u128>()],
        &[],
        Fixtures::unreachable,
    );

    #[tokio::test]
    async fn runs_functions_once_their_resources_exist_in_any_order() {
        let mut resources = Resources::new();
        Provider::resolve_all(&mut resources, vec![&TEXT, &NUMBER])
            .await
            .unwrap();
        assert_eq!(resources.get::<String>().unwrap(), "number 7");
    }

    #[tokio::test]
    async fn a_type_inserted_by_hand_skips_its_function() {
        let mut resources = Resources::new();
        resources.insert(5_u64);
        Provider::resolve_all(&mut resources, vec![&TEXT, &NUMBER])
            .await
            .unwrap();
        assert_eq!(resources.get::<String>().unwrap(), "number 5");
    }

    #[tokio::test]
    async fn an_optional_resource_waits_for_its_function() {
        let mut resources = Resources::new();
        Provider::resolve_all(&mut resources, vec![&GREETING, &NUMBER])
            .await
            .unwrap();
        assert_eq!(resources.get::<String>().unwrap(), "Some(7)");
    }

    #[tokio::test]
    async fn an_optional_resource_nobody_provides_is_none() {
        let mut resources = Resources::new();
        Provider::resolve_all(&mut resources, vec![&GREETING])
            .await
            .unwrap();
        assert_eq!(resources.get::<String>().unwrap(), "None");
    }

    #[tokio::test]
    async fn functions_waiting_only_on_each_other_optionally_still_run() {
        let mut resources = Resources::new();
        Provider::resolve_all(&mut resources, vec![&PING, &PONG])
            .await
            .unwrap();
        assert_eq!(resources.get::<u8>(), Some(1));
        assert_eq!(resources.get::<u16>(), Some(20));
    }

    #[tokio::test]
    async fn functions_that_cannot_run_name_what_they_miss() {
        let error = Provider::resolve_all(&mut Resources::new(), vec![&TEXT])
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "resource functions cannot run: tests::text needs u64, which was never inserted, and no #[haze::resource] provides it"
        );
    }

    #[tokio::test]
    async fn two_functions_for_one_type_are_rejected() {
        let error = Provider::resolve_all(&mut Resources::new(), vec![&NUMBER, &OTHER_NUMBER])
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "u64 is provided by more than one function: tests::number, tests::other_number"
        );
    }

    #[tokio::test]
    async fn several_duplicated_types_are_listed_in_order() {
        let error = Provider::resolve_all(
            &mut Resources::new(),
            vec![&OTHER_NUMBER, &NUMBER, &TEXT, &GREETING],
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "alloc::string::String is provided by more than one function: tests::text, tests::greeting; u64 is provided by more than one function: tests::other_number, tests::number"
        );
    }

    #[tokio::test]
    async fn a_type_needed_twice_is_named_once() {
        let error = Provider::resolve_all(&mut Resources::new(), vec![&TWICE])
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "resource functions cannot run: tests::twice needs u128, which was never inserted, and no #[haze::resource] provides it"
        );
    }

    #[tokio::test]
    async fn two_functions_for_one_type_are_rejected_even_when_it_was_inserted() {
        let mut resources = Resources::new();
        resources.insert(5_u64);
        let error = Provider::resolve_all(&mut resources, vec![&NUMBER, &OTHER_NUMBER])
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("u64 is provided by more than one function")
        );
    }

    #[tokio::test]
    async fn a_failing_function_stops_startup_with_its_name() {
        let error = Provider::resolve_all(&mut Resources::new(), vec![&FAILING])
            .await
            .unwrap_err();
        assert_eq!(
            format!("{error:#}"),
            "tests::failing failed to provide u32: disk is full"
        );
    }

    #[tokio::test]
    async fn a_required_cycle_suggests_later() {
        let error = Provider::resolve_all(
            &mut Resources::new(),
            vec![&FIRST_OF_CYCLE, &SECOND_OF_CYCLE],
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "resource functions cannot run: tests::first needs i16, which only tests::second provides, and tests::second needs i8 back; take one of them as Later<T> to break the cycle"
        );
    }

    #[tokio::test]
    async fn a_longer_required_cycle_names_every_function_and_suggests_later() {
        let error = Provider::resolve_all(
            &mut Resources::new(),
            vec![&START_OF_RING, &MIDDLE_OF_RING, &END_OF_RING],
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "resource functions cannot run: tests::end needs char, which only tests::start provides, and tests::start needs bool, which only tests::middle provides, and tests::middle needs f32 back; take one of them as Later<T> to break the cycle"
        );
    }

    #[tokio::test]
    async fn overlapping_required_cycles_are_each_reported_once() {
        let error = Provider::resolve_all(&mut Resources::new(), vec![&KNOT_A, &KNOT_B, &KNOT_C])
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "resource functions cannot run: tests::knot_a needs alloc::vec::Vec<i16>, which only tests::knot_b provides, and tests::knot_b needs alloc::vec::Vec<i8> back; take one of them as Later<T> to break the cycle; tests::knot_b needs alloc::vec::Vec<i32>, which only tests::knot_c provides, and tests::knot_c needs alloc::vec::Vec<i8>, which only tests::knot_a provides, and tests::knot_a needs alloc::vec::Vec<i16> back; take one of them as Later<T> to break the cycle"
        );
    }

    #[tokio::test]
    async fn a_type_another_function_will_provide_is_not_named_as_missing() {
        let error = Provider::resolve_all(
            &mut Resources::new(),
            vec![&NEEDS_NUMBER_AND_MISSING, &NUMBER, &BLOCKED],
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "resource functions cannot run: tests::blocked needs u128, which was never inserted, and no #[haze::resource] provides it; tests::needs_number_and_missing needs u128, which was never inserted, and no #[haze::resource] provides it"
        );
    }

    #[tokio::test]
    async fn a_missing_requirement_stops_startup_before_any_optional_fallback() {
        let mut resources = Resources::new();
        let error = Provider::resolve_all(&mut resources, vec![&WAITS_ON_BLOCKED, &BLOCKED])
            .await
            .unwrap_err();
        assert!(error.to_string().contains("tests::blocked needs u128"));
        assert!(!resources.contains::<i32>());
    }

    #[tokio::test]
    async fn an_unsatisfiable_function_stops_startup_before_an_unrelated_one_runs() {
        let mut resources = Resources::new();
        let error = Provider::resolve_all(&mut resources, vec![&NUMBER, &BLOCKED])
            .await
            .unwrap_err();
        assert!(error.to_string().contains("tests::blocked needs u128"));
        assert!(!resources.contains::<u64>());
    }

    #[tokio::test]
    async fn a_function_behind_a_blocked_one_says_so() {
        let error = Provider::resolve_all(&mut Resources::new(), vec![&AFTER_BLOCKED, &BLOCKED])
            .await
            .unwrap_err();
        assert!(error.to_string().contains(
            "tests::after_blocked needs i64, which only tests::blocked provides, and tests::blocked cannot run either"
        ));
    }

    #[tokio::test]
    async fn only_a_function_on_the_cycle_runs_without_its_optional() {
        let mut resources = Resources::new();
        Provider::resolve_all(&mut resources, vec![&OUTSIDER, &PING, &PONG])
            .await
            .unwrap();
        assert_eq!(resources.get::<i128>(), Some(1));
        assert_eq!(resources.get::<u8>(), Some(1));
        assert_eq!(resources.get::<u16>(), Some(20));
    }

    #[tokio::test]
    async fn a_required_cycle_stops_startup_before_anything_runs() {
        let mut resources = Resources::new();
        let error = Provider::resolve_all(
            &mut resources,
            vec![&WAITS_ON_CYCLE, &FIRST_OF_CYCLE, &SECOND_OF_CYCLE, &NUMBER],
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("take one of them as Later<T>"));
        assert!(!resources.contains::<i32>());
        assert!(!resources.contains::<u64>());
    }

    #[tokio::test]
    async fn a_longer_optional_cycle_gives_up_only_in_its_first_function() {
        let mut resources = Resources::new();
        Provider::resolve_all(
            &mut resources,
            vec![&HEAD_OF_RING, &BODY_OF_RING, &TAIL_OF_RING],
        )
        .await
        .unwrap();
        assert_eq!(resources.get::<Vec<u8>>(), Some(vec![1]));
        assert_eq!(resources.get::<Vec<u16>>(), Some(vec![2]));
        assert_eq!(resources.get::<Vec<u32>>(), Some(vec![2]));
    }

    #[tokio::test]
    async fn a_function_on_one_cycle_still_gets_the_types_of_another_cycle() {
        let mut resources = Resources::new();
        Provider::resolve_all(&mut resources, vec![&BRIDGE, &ECHO, &PING, &PONG])
            .await
            .unwrap();
        assert_eq!(resources.get::<isize>(), Some(10));
        assert_eq!(resources.get::<&'static str>(), Some("after"));
        assert_eq!(resources.get::<u8>(), Some(1));
        assert_eq!(resources.get::<u16>(), Some(20));
    }

    #[tokio::test]
    async fn which_function_gives_up_does_not_depend_on_where_another_cycle_sorts() {
        let mut resources = Resources::new();
        Provider::resolve_all(&mut resources, vec![&PING, &PONG, &BRIDGE, &ECHO])
            .await
            .unwrap();
        assert_eq!(resources.get::<isize>(), Some(10));
        assert_eq!(resources.get::<&'static str>(), Some("after"));
    }

    #[tokio::test]
    async fn a_cycle_of_one_required_and_one_optional_runs_the_optional_side_first() {
        let mut resources = Resources::new();
        Provider::resolve_all(&mut resources, vec![&NEEDS_BACK, &GIVES_BACK])
            .await
            .unwrap();
        assert_eq!(resources.get::<f64>(), Some(1.0));
        assert_eq!(resources.get::<usize>(), Some("back 1".len()));
    }

    #[test]
    fn debug_names_the_function_and_its_types() {
        assert_eq!(
            format!("{TEXT:?}"),
            "Provider { name: \"tests::text\", provides: alloc::string::String, needs: [u64], optional: [], .. }"
        );
    }
}
