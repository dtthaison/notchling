use super::folders::expand;
use super::preview::{preview, Lookup};
use super::tests_support::Rig;
use super::{Document, EditError, Kind, Moved, Overlap, Problem, Relation, PERSONAL_ID};
use std::path::{Path, PathBuf};

fn doc() -> Document {
    let mut doc = Document::personal_only();
    doc.create("AptusFIT", None, Some("orange")).unwrap();
    doc.create("Acme", None, Some("blue")).unwrap();
    doc.set_folders("aptusfit", &["~/work/aptusfit".to_owned()])
        .unwrap();
    doc
}

fn run(doc: &Document, id: &str, wanted: &[&str], sessions: &[&str]) -> super::FolderPreview {
    let wanted: Vec<String> = wanted.iter().map(|f| (*f).to_owned()).collect();
    let sessions: Vec<PathBuf> = sessions.iter().map(PathBuf::from).collect();
    let none = |_: &Path| None;
    let key = |f: &str| {
        expand(f, Path::new("/Users/me"))
            .map_or_else(|| f.to_owned(), |p| p.to_string_lossy().into_owned())
    };
    let look = Lookup {
        home: Path::new("/Users/me"),
        resolve: &none,
        fold: false,
        key: &key,
    };
    preview(doc, id, &wanted, &sessions, &look)
}

#[test]
fn a_new_folder_lists_the_sessions_that_would_change_organization() {
    let out = run(
        &doc(),
        "acme",
        &["/srv/acme"],
        &[
            "/srv/acme/app",
            "/srv/acme/app",
            "/srv/other",
            "/Users/me/work/aptusfit/fe",
        ],
    );
    assert_eq!(out.problem, None);
    assert_eq!(out.folders, ["/srv/acme"]);
    assert_eq!(
        out.moved,
        [Moved {
            folder: "/srv/acme/app".into(),
            from: PERSONAL_ID.into(),
            to: "acme".into()
        }]
    );
    assert!(out.overlaps.is_empty());
}

#[test]
fn removing_a_folder_sends_its_sessions_back_to_personal() {
    let out = run(&doc(), "aptusfit", &[], &["/Users/me/work/aptusfit/fe"]);
    assert_eq!(out.folders, Vec::<String>::new());
    assert_eq!(
        out.moved,
        [Moved {
            folder: "/Users/me/work/aptusfit/fe".into(),
            from: "aptusfit".into(),
            to: PERSONAL_ID.into()
        }]
    );
}

#[test]
fn a_folder_inside_another_organizations_folder_is_allowed_and_flagged() {
    let out = run(
        &doc(),
        "acme",
        &["~/work/aptusfit/vendor/acme"],
        &[
            "/Users/me/work/aptusfit/vendor/acme/x",
            "/Users/me/work/aptusfit/fe",
        ],
    );
    assert_eq!(out.problem, None);
    assert_eq!(
        out.overlaps,
        [Overlap {
            folder: "~/work/aptusfit/vendor/acme".into(),
            organization: "aptusfit".into(),
            other: "~/work/aptusfit".into(),
            relation: Relation::Inside,
        }]
    );
    assert_eq!(
        out.moved,
        [Moved {
            folder: "/Users/me/work/aptusfit/vendor/acme/x".into(),
            from: "aptusfit".into(),
            to: "acme".into()
        }]
    );
}

#[test]
fn a_folder_that_holds_another_organizations_folder_leaves_that_one_alone() {
    let out = run(
        &doc(),
        "acme",
        &["~/work"],
        &["/Users/me/work/aptusfit/fe", "/Users/me/work/notes"],
    );
    assert_eq!(
        out.overlaps,
        [Overlap {
            folder: "~/work".into(),
            organization: "aptusfit".into(),
            other: "~/work/aptusfit".into(),
            relation: Relation::Contains,
        }]
    );
    assert_eq!(
        out.moved,
        [Moved {
            folder: "/Users/me/work/notes".into(),
            from: PERSONAL_ID.into(),
            to: "acme".into()
        }]
    );
}

#[test]
fn the_same_folder_in_two_organizations_is_refused_with_who_has_it() {
    let out = run(&doc(), "acme", &["/Users/me/work/aptusfit"], &[]);
    assert_eq!(
        out.problem,
        Some(Problem::FolderTaken {
            folder: "/Users/me/work/aptusfit".into(),
            by: "aptusfit".into()
        })
    );
    assert!(out.moved.is_empty());
    let out = run(&doc(), "acme", &["relative/dir"], &[]);
    assert_eq!(
        out.problem,
        Some(Problem::FolderInvalid("relative/dir".into()))
    );
    let out = run(&doc(), "nobody", &["/srv/x"], &[]);
    assert_eq!(out.problem, Some(Problem::IdUnknown("nobody".into())));
}

#[test]
fn the_preview_writes_nothing() {
    let rig = Rig::new("preview-writes-nothing");
    let home = rig.tmp.path("home");
    std::fs::create_dir_all(home.join("work/acme")).unwrap();
    let orgs = rig.orgs.clone().with_home(&home);
    orgs.create("Acme", None, None).unwrap();
    let before = rig.orgs_text();
    let session = home.join("work/acme/app");
    std::fs::create_dir_all(&session).unwrap();
    let out = orgs
        .preview_folders(
            "acme",
            &["~/work/acme".to_owned()],
            std::slice::from_ref(&session),
        )
        .unwrap();
    assert_eq!(out.moved.len(), 1);
    assert_eq!(out.moved[0].to, "acme");
    assert_eq!(rig.orgs_text(), before);
    assert_eq!(orgs.view().organizations()[1].folders, Vec::<String>::new());
}

#[test]
fn an_unreadable_document_gives_no_preview() {
    let rig = Rig::new("preview-unreadable");
    crate::store::put_doc(&rig.data, super::DOC, "{not json").unwrap();
    let err = rig
        .orgs
        .preview_folders("personal", &["/srv/x".to_owned()], &[])
        .unwrap_err();
    assert!(matches!(err, EditError::Unreadable(_)));
    let _ = Kind::Claude;
}

#[cfg(unix)]
#[test]
fn a_folder_reached_through_a_link_overlaps_the_folder_it_links_to() {
    let rig = Rig::new("preview-symlink");
    let home = rig.tmp.path("home");
    let real = rig.tmp.path("real/aptus");
    std::fs::create_dir_all(real.join("vendor")).unwrap();
    std::fs::create_dir_all(home.join("work")).unwrap();
    std::os::unix::fs::symlink(&real, home.join("work/aptus")).unwrap();
    let orgs = rig.orgs.clone().with_home(&home);
    orgs.create("AptusFIT", None, None).unwrap();
    orgs.create("Acme", None, None).unwrap();
    orgs.set_folders("aptusfit", &["~/work/aptus".to_owned()])
        .unwrap();
    let wanted = [real.join("vendor").to_string_lossy().into_owned()];
    let out = orgs.preview_folders("acme", &wanted, &[]).unwrap();
    assert_eq!(out.problem, None);
    assert_eq!(
        out.overlaps,
        [Overlap {
            folder: wanted[0].clone(),
            organization: "aptusfit".into(),
            other: "~/work/aptus".into(),
            relation: Relation::Inside,
        }]
    );
}
