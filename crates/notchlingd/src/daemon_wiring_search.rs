use crate::daemon_wiring::Wired;
use crate::layout::Layout;
use crate::pages_ops::{PagesOps, PagesSetup, SearchSettings};
use std::sync::Arc;

pub fn pages(wired: &Wired, layout: &Layout) -> Arc<PagesOps> {
    let settings_file = layout.data_dir.join("settings.json");
    let setup = PagesSetup::from_layout(layout);
    let accounts = crate::search_accounts::resolver(setup.roots.clone(), layout.data_dir.clone());
    let pages = Arc::new(
        PagesOps::new(setup)
            .with_accounts(accounts)
            .with_titles(wired.agy.titles())
            .with_search_settings(Arc::new(move || SearchSettings::read(&settings_file))),
    );
    let embed = crate::daemon_wiring_embed::build(wired, layout, &pages);
    let _ = wired.embed.set(embed);
    let poked = pages.clone();
    wired
        .settings
        .bind_search_poke(Arc::new(move || poked.poke()));
    pages
}
