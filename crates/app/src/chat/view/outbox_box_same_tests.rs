use super::*;

#[test]
fn a_box_holding_this_very_message_has_its_own_line() {
    assert_eq!(
        status_line(&Status::BoxHoldsThis),
        "This message is already in the box"
    );
    assert_ne!(
        status_line(&Status::BoxHoldsThis),
        status_line(&Status::BoxHasText(String::new()))
    );
}
