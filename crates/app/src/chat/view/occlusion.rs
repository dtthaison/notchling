use gpui::{Hitbox, Window};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit<K> {
    Text(K),
    Covered,
    Empty,
}

pub fn reachable(hitbox: Option<&Hitbox>, window: &Window) -> bool {
    hitbox.is_none_or(|hitbox| hitbox.is_hovered(window))
}

pub fn pick<K: Clone>(candidates: &[(K, bool, bool)]) -> Hit<K> {
    let mut covered = false;
    for (key, contains, reachable) in candidates {
        if !*contains {
            continue;
        }
        if *reachable {
            return Hit::Text(key.clone());
        }
        covered = true;
    }
    if covered {
        Hit::Covered
    } else {
        Hit::Empty
    }
}

pub fn copy_text(overlay_open: bool, behind: impl FnOnce() -> Option<String>) -> Option<String> {
    if overlay_open {
        None
    } else {
        behind()
    }
}

#[cfg(test)]
#[path = "occlusion_tests.rs"]
mod tests;
