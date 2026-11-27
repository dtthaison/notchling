#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Sessions,
    Settings,
    Pages,
    Inbox,
    Actions,
    Activity,
    Vault,
    Machines,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Sessions,
        Category::Settings,
        Category::Pages,
        Category::Inbox,
        Category::Actions,
        Category::Activity,
        Category::Vault,
        Category::Machines,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Category::Sessions => "sessions",
            Category::Settings => "settings",
            Category::Pages => "pages",
            Category::Inbox => "inbox",
            Category::Actions => "actions",
            Category::Activity => "activity",
            Category::Vault => "vault",
            Category::Machines => "machines",
        }
    }

    pub fn from_id(id: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.id() == id)
    }

    pub fn default_on(self) -> bool {
        matches!(
            self,
            Category::Sessions
                | Category::Settings
                | Category::Pages
                | Category::Inbox
                | Category::Actions
        )
    }

    pub fn cap(self) -> usize {
        if self == Category::Sessions {
            8
        } else {
            4
        }
    }

    pub fn in_daemon(self) -> bool {
        matches!(
            self,
            Category::Sessions
                | Category::Pages
                | Category::Activity
                | Category::Vault
                | Category::Machines
        )
    }

    pub fn app_tier(self) -> bool {
        matches!(
            self,
            Category::Activity | Category::Vault | Category::Machines
        )
    }
}

pub fn order(stored: &[String]) -> Vec<Category> {
    let mut out: Vec<Category> = Vec::new();
    for id in stored {
        if let Some(c) = Category::from_id(id) {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    for c in Category::ALL {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

pub fn on(stored: &[String]) -> Vec<Category> {
    let mut out: Vec<Category> = Vec::new();
    for id in stored {
        if let Some(c) = Category::from_id(id) {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}
