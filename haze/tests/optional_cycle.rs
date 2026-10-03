use haze::Resources;

#[derive(Clone, Debug, PartialEq)]
struct Left(Option<bool>);

#[derive(Clone, Debug, PartialEq)]
struct Right(Option<bool>);

#[haze::resource]
fn a_left(right: Option<Right>) -> Left {
    Left(right.map(|right| right.0.is_some()))
}

#[haze::resource]
fn b_right(left: Option<Left>) -> Right {
    Right(left.map(|left| left.0.is_some()))
}

#[tokio::test]
async fn the_first_function_by_name_gives_up_its_optional() {
    let mut resources = Resources::new();
    resources.provide().await.unwrap();
    assert_eq!(resources.get::<Left>(), Some(Left(None)));
    assert_eq!(resources.get::<Right>(), Some(Right(Some(false))));
}
