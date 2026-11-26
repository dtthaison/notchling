use super::tab_reveal::{Reveal, Step, MAX_TRIES};

#[test]
fn a_reveal_waits_until_the_row_has_been_laid_out_then_applies() {
    let mut reveal = Reveal::new(4);
    assert_eq!(reveal.step(false), Step::Retry);
    assert_eq!(reveal.step(true), Step::Apply);
}

#[test]
fn a_reveal_that_never_gets_a_layout_gives_up_after_a_few_frames() {
    let mut reveal = Reveal::new(2);
    let mut steps = 0;
    while reveal.step(false) == Step::Retry {
        steps += 1;
        assert!(steps <= MAX_TRIES as usize);
    }
    assert_eq!(steps, MAX_TRIES as usize);
    assert_eq!(reveal.step(false), Step::Drop);
}
