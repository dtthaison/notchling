use notchling_engine::reply::exec::Failure;
pub(crate) use notchling_engine::terminal::input_box::masked;

pub(crate) fn head(
    job: &str,
    failure: &Failure,
    chars: usize,
    cursor: Option<(usize, usize)>,
    trail: &[String],
) -> Vec<String> {
    let cursor = match cursor {
        Some((row, col)) => format!("cursor: row {row}, column {col}"),
        None => "cursor: hidden".to_string(),
    };
    let mut head = vec![
        format!("job: {job}"),
        format!("refused: {failure}"),
        format!("message: {chars} characters"),
        cursor,
        format!("----- what each step saw ({} notes) -----", trail.len()),
    ];
    head.extend(trail.iter().cloned());
    head
}
