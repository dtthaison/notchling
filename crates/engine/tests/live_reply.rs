fn live() -> bool {
    let on = std::env::var("NOTCHLING_LIVE").is_ok_and(|v| v == "1");
    if !on {
        println!("live test off: set NOTCHLING_LIVE=1 (and NOTCHLING_LIVE_JOB for most)");
    }
    on
}

fn job() -> Option<String> {
    std::env::var("NOTCHLING_LIVE_JOB")
        .ok()
        .filter(|v| !v.is_empty())
}

#[test]
fn live_send_reply() {
    if !live() {
        return;
    }
    assert!(job().is_some(), "NOTCHLING_LIVE_JOB is needed");
    println!("live_send_reply: waiting for E3 (reply typing)");
}

#[test]
fn live_refuses_reply_into_a_dialog() {
    if !live() {
        return;
    }
    assert!(
        std::env::var("NOTCHLING_LIVE_ASK_JOB").is_ok(),
        "NOTCHLING_LIVE_ASK_JOB is needed"
    );
    println!("live_refuses_reply_into_a_dialog: waiting for E3 (dialog check)");
}

#[test]
fn live_reads_and_answers_a_question() {
    if !live() {
        return;
    }
    assert!(
        std::env::var("NOTCHLING_LIVE_ASK_JOB").is_ok(),
        "NOTCHLING_LIVE_ASK_JOB is needed"
    );
    println!("live_reads_and_answers_a_question: waiting for E3 (question reading)");
}

#[test]
fn live_terminal_menu_and_resize() {
    if !live() {
        return;
    }
    assert!(job().is_some(), "NOTCHLING_LIVE_JOB is needed");
    println!("live_terminal_menu_and_resize: waiting for E3 (pty host)");
}

#[test]
fn live_interrupt_only_while_working() {
    if !live() {
        return;
    }
    assert!(job().is_some(), "NOTCHLING_LIVE_JOB is needed");
    println!("live_interrupt_only_while_working: waiting for E3 (interrupt)");
}

#[test]
fn live_take_back_queued() {
    if !live() {
        return;
    }
    assert!(job().is_some(), "NOTCHLING_LIVE_JOB is needed");
    assert!(
        std::env::var("NOTCHLING_LIVE_TRANSCRIPT").is_ok(),
        "NOTCHLING_LIVE_TRANSCRIPT is needed"
    );
    println!("live_take_back_queued: waiting for E3 (queue)");
}
