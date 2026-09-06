use writ_core::Error;

use crate::ui::AppState;

pub fn render(
    _state: &AppState,
    _q: Option<&str>,
    _sort: Option<&str>,
    _dir: Option<&str>,
    _status: Option<&str>,
) -> Result<String, Error> {
    Ok(r#"<div class="shell">Collection is empty.</div>"#.into())
}
