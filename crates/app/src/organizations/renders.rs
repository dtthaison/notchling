use notchling_engine::organizations::COLORS;
use serde_json::{json, Value};

use super::model::{Load, Open, OrganizationsModel};
use crate::views::ViewSpec;

pub const HOME: &str = "/Users/me";

pub const VIEWS: [ViewSpec; 6] = [
    ViewSpec {
        name: "settings-organizations",
        size: (620.0, 520.0),
        about: "the Settings sheet's Organizations page: Personal, AptusFIT and an example company, each with its colour, counts and folders (made-up data)",
    },
    ViewSpec {
        name: "settings-organizations-one",
        size: (620.0, 380.0),
        about: "the Organizations page with only Personal: the list and New Organization… (made-up data)",
    },
    ViewSpec {
        name: "settings-organizations-new",
        size: (620.0, 740.0),
        about: "the Organizations page with the New Organization form: a name already taken and a colour chosen (made-up data)",
    },
    ViewSpec {
        name: "settings-organizations-menu",
        size: (620.0, 520.0),
        about: "the Organizations page with an organization's menu open (made-up data)",
    },
    ViewSpec {
        name: "settings-organizations-folders",
        size: (620.0, 880.0),
        about: "the Organizations page with the folder editor open: a folder inside another organization's folder, its overlap and the open sessions that would change (made-up data)",
    },
    ViewSpec {
        name: "settings-organizations-refused",
        size: (620.0, 520.0),
        about: "the Organizations page after Delete Organization on one that still holds accounts and secrets (made-up data)",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    List,
    One,
    New,
    Menu,
    Folders,
    Refused,
}

pub fn shown(name: &str) -> Option<Shown> {
    Some(match name {
        "settings-organizations" => Shown::List,
        "settings-organizations-one" => Shown::One,
        "settings-organizations-new" => Shown::New,
        "settings-organizations-menu" => Shown::Menu,
        "settings-organizations-folders" => Shown::Folders,
        "settings-organizations-refused" => Shown::Refused,
        _ => return None,
    })
}

fn org(id: &str, name: &str, color: &str, folders: &[&str], counts: (u64, u64, u64)) -> Value {
    json!({
        "id": id, "name": name, "githubOwner": null, "color": color,
        "folders": folders, "personal": id == "personal",
        "counts": {"accounts": counts.0, "secrets": counts.1, "machines": counts.2},
    })
}

pub fn reply(shown: Shown) -> Value {
    let personal = org("personal", "Personal", "grey", &[], (2, 3, 2));
    if shown == Shown::One {
        return json!({"organizations": [personal], "colors": COLORS,
            "onlyPersonal": true, "unreadable": null});
    }
    let aptusfit = org(
        "aptusfit",
        "AptusFIT",
        "orange",
        &["~/work/aptusfit"],
        (2, 3, 1),
    );
    let acme = org(
        "acme-studio",
        "Acme Studio",
        "blue",
        &["~/work/acme", "~/clients/acme-site"],
        (1, 1, 0),
    );
    json!({"organizations": [personal, aptusfit, acme], "colors": COLORS,
        "onlyPersonal": false, "unreadable": null})
}

pub fn preview_reply() -> Value {
    json!({
        "folders": ["~/work/acme", "~/clients/acme-site", "~/work/aptusfit/vendor/acme-sdk"],
        "problem": null,
        "problemText": null,
        "overlaps": [{"folder": "~/work/aptusfit/vendor/acme-sdk", "organization": "aptusfit",
            "other": "~/work/aptusfit", "relation": "inside"}],
        "moved": [
            {"folder": "/Users/me/work/aptusfit/vendor/acme-sdk", "from": "aptusfit", "to": "acme-studio"},
            {"folder": "/Users/me/work/aptusfit/vendor/acme-sdk/examples", "from": "aptusfit", "to": "acme-studio"},
        ],
    })
}

pub fn model(shown: Shown) -> OrganizationsModel {
    let mut model = OrganizationsModel {
        home: HOME.to_owned(),
        ..OrganizationsModel::default()
    };
    model.finish(Ok(reply(shown)));
    model.load = Load::Idle;
    match shown {
        Shown::List | Shown::One => {}
        Shown::New => {
            model.begin_new();
            model.new_color = Some("teal".to_owned());
            model.fail(
                None,
                "",
                Some(&json!({"problems": [{"kind": "name_taken", "name": "Acme Studio"}]})),
            );
        }
        Shown::Menu => model.toggle_menu("acme-studio"),
        Shown::Folders => {
            model.begin_folders("acme-studio");
            let drafts = model.drafts().map(<[String]>::to_vec).unwrap_or_default();
            let mut drafts = drafts;
            drafts.push("~/work/aptusfit/vendor/acme-sdk".to_owned());
            if let Open::Folders(edit) = &mut model.open {
                edit.drafts = drafts;
            }
            let preview = model.parse_preview(&preview_reply());
            if let Open::Folders(edit) = &mut model.open {
                edit.preview = preview;
            }
        }
        Shown::Refused => model.ask_delete("aptusfit"),
    }
    model
}

pub fn texts(shown: Shown) -> (&'static str, &'static str) {
    match shown {
        Shown::New => ("Acme Studio", ""),
        Shown::Folders => ("", "~/clients/"),
        _ => ("", ""),
    }
}
