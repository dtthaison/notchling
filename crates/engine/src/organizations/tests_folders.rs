use super::folders::{expand, find, standardize};
use super::{normalize_folder, Document, Organization, PERSONAL_ID};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn org(id: &str, folders: &[&str]) -> Organization {
    Organization {
        id: id.into(),
        name: id.to_uppercase(),
        folders: folders.iter().map(|f| (*f).to_owned()).collect(),
        ..Organization::personal()
    }
}

fn links(pairs: &[(&str, &str)]) -> impl Fn(&Path) -> Option<PathBuf> {
    let map: HashMap<PathBuf, PathBuf> = pairs
        .iter()
        .map(|(a, b)| (PathBuf::from(a), PathBuf::from(b)))
        .collect();
    move |p: &Path| map.get(p).cloned()
}

fn id(orgs: &[Organization], folder: &str, resolve: &dyn Fn(&Path) -> Option<PathBuf>) -> String {
    find(orgs, Path::new(folder), Path::new("/Users/me"), resolve).organization
}

#[test]
fn a_folder_belongs_to_the_organization_whose_folder_holds_it() {
    let orgs = [
        org(PERSONAL_ID, &[]),
        org("aptusfit", &["~/work/aptusfit"]),
        org("acme", &["/srv/acme"]),
    ];
    let none = links(&[]);
    assert_eq!(id(&orgs, "/Users/me/work/aptusfit", &none), "aptusfit");
    assert_eq!(
        id(&orgs, "/Users/me/work/aptusfit/fe/src", &none),
        "aptusfit"
    );
    assert_eq!(
        id(&orgs, "/Users/me/work/aptusfit/../aptusfit/fe", &none),
        "aptusfit"
    );
    assert_eq!(id(&orgs, "/Users/me/work/aptusfit/", &none), "aptusfit");
    assert_eq!(id(&orgs, "/srv/acme/app", &none), "acme");
    assert_eq!(
        id(&orgs, "/Users/me/work/aptusfit2", &none),
        PERSONAL_ID,
        "a name that only starts the same"
    );
    assert_eq!(id(&orgs, "/Users/me/work", &none), PERSONAL_ID);
    assert_eq!(id(&orgs, "/elsewhere", &none), PERSONAL_ID);
    assert_eq!(
        id(&orgs, "work/aptusfit", &none),
        PERSONAL_ID,
        "a relative folder places nothing"
    );
    assert_eq!(id(&orgs, "", &none), PERSONAL_ID);
}

#[test]
fn the_longest_folder_wins_wherever_it_is_listed() {
    let none = links(&[]);
    let orgs = [
        org(PERSONAL_ID, &["~/work"]),
        org("aptusfit", &["~/work/aptusfit"]),
        org("deploy", &["~/work/aptusfit/deploy"]),
    ];
    assert_eq!(id(&orgs, "/Users/me/work/x", &none), PERSONAL_ID);
    assert_eq!(id(&orgs, "/Users/me/work/aptusfit/x", &none), "aptusfit");
    assert_eq!(
        id(&orgs, "/Users/me/work/aptusfit/deploy/x", &none),
        "deploy"
    );
    let flipped = [orgs[2].clone(), orgs[1].clone(), orgs[0].clone()];
    assert_eq!(
        id(&flipped, "/Users/me/work/aptusfit/deploy/x", &none),
        "deploy"
    );
    assert_eq!(id(&flipped, "/Users/me/work/aptusfit/x", &none), "aptusfit");
    let found = find(
        &orgs,
        Path::new("/Users/me/work/aptusfit/x"),
        Path::new("/Users/me"),
        &none,
    );
    assert_eq!(found.folder.as_deref(), Some("~/work/aptusfit"));
}

#[test]
fn a_link_to_a_folder_matches_through_the_file_system() {
    let orgs = [org(PERSONAL_ID, &[]), org("aptusfit", &["~/work/aptusfit"])];
    let resolve = links(&[
        ("/tmp/aptus", "/Users/me/work/aptusfit"),
        ("/tmp/aptus/fe", "/Users/me/work/aptusfit/fe"),
        ("/Users/me/work/aptusfit", "/Users/me/work/aptusfit"),
    ]);
    assert_eq!(id(&orgs, "/tmp/aptus/fe", &resolve), "aptusfit");
    assert_eq!(id(&orgs, "/tmp/other", &resolve), PERSONAL_ID);

    let moved = [org(PERSONAL_ID, &[]), org("aptusfit", &["~/link"])];
    let resolve = links(&[
        ("/Users/me/link", "/data/aptus"),
        ("/data/aptus/app", "/data/aptus/app"),
    ]);
    assert_eq!(
        id(&moved, "/data/aptus/app", &resolve),
        "aptusfit",
        "the folder is a link, the session is in its target"
    );
    assert_eq!(id(&moved, "/data/other", &resolve), PERSONAL_ID);
}

#[cfg(unix)]
#[test]
fn the_real_file_system_resolves_a_link_the_same_way() {
    use crate::sessions::testutil::TempDir;
    let tmp = TempDir::new("org-folders-real");
    let real = tmp.path("real/aptus");
    std::fs::create_dir_all(real.join("app")).unwrap();
    std::fs::create_dir_all(tmp.path("home/work")).unwrap();
    std::os::unix::fs::symlink(&real, tmp.path("home/work/aptus")).unwrap();
    let orgs = [org(PERSONAL_ID, &[]), org("aptusfit", &["~/work/aptus"])];
    let home = tmp.path("home");
    let by = |folder: &Path| find(&orgs, folder, &home, &super::folders::real).organization;
    assert_eq!(by(&real.join("app")), "aptusfit");
    assert_eq!(by(&tmp.path("home/work/aptus/app")), "aptusfit");
    assert_eq!(by(&tmp.path("home/work")), PERSONAL_ID);
    assert_eq!(by(&tmp.path("gone/aptus/app")), PERSONAL_ID);
}

#[test]
fn the_home_marker_and_the_normal_form() {
    let home = Path::new("/Users/me");
    assert_eq!(expand("~", home), Some(PathBuf::from("/Users/me")));
    assert_eq!(
        expand("~/a/./b/", home),
        Some(PathBuf::from("/Users/me/a/b"))
    );
    assert_eq!(expand("/a//b/../c", home), Some(PathBuf::from("/a/c")));
    assert_eq!(expand("a/b", home), None);
    assert_eq!(expand("/", home), None);
    assert_eq!(expand("~/..", home), None);
    assert_eq!(expand("~other/x", home), None);
    assert_eq!(standardize(Path::new("rel")), None);
    assert_eq!(normalize_folder(" ~/a/b/ ").as_deref(), Some("~/a/b"));
    assert_eq!(normalize_folder("~").as_deref(), Some("~"));
    assert_eq!(normalize_folder("/x/../y").as_deref(), Some("/y"));
    assert_eq!(normalize_folder("/x/../.."), None);
    assert_eq!(normalize_folder("/"), None);
    assert_eq!(normalize_folder(""), None);
    assert_eq!(normalize_folder("/a\nb"), None);
}

#[test]
fn a_document_places_a_folder_through_its_view() {
    let mut doc = Document::personal_only();
    let id = doc.create("AptusFIT", None, None).unwrap();
    doc.set_folders(&id, &["~/work/aptusfit".to_owned()])
        .unwrap();
    let view = super::View::new(super::State::Loaded(doc));
    let none = links(&[]);
    let found = view.folder(
        Path::new("/Users/me/work/aptusfit/fe"),
        Path::new("/Users/me"),
        &none,
    );
    assert_eq!(found.organization, "aptusfit");
    assert_eq!(
        view.folder(Path::new("/Users/me/code"), Path::new("/Users/me"), &none)
            .organization,
        PERSONAL_ID
    );
}

#[test]
fn the_deeper_real_folder_wins_over_a_link_that_looks_deeper_and_a_tie_is_settled_by_id() {
    let orgs = [
        org(PERSONAL_ID, &[]),
        org("outer", &["~/link"]),
        org("inner", &["/data/proj/sub"]),
    ];
    let resolve = links(&[
        ("/Users/me/link", "/data/proj"),
        ("/Users/me/link/sub", "/data/proj/sub"),
        ("/data/proj/sub", "/data/proj/sub"),
    ]);
    assert_eq!(id(&orgs, "/Users/me/link/sub", &resolve), "inner");

    let tied = [
        org(PERSONAL_ID, &[]),
        org("zeta", &["/srv/x"]),
        org("alpha", &["/srv/x"]),
    ];
    let none = links(&[]);
    assert_eq!(id(&tied, "/srv/x/app", &none), "alpha");
    let flipped = [
        org(PERSONAL_ID, &[]),
        org("alpha", &["/srv/x"]),
        org("zeta", &["/srv/x"]),
    ];
    assert_eq!(id(&flipped, "/srv/x/app", &none), "alpha");
}

#[test]
fn a_session_matches_a_folder_in_any_case_only_where_the_disk_ignores_case() {
    let orgs = [org(PERSONAL_ID, &[]), org("acme", &["/Srv/X"])];
    let none = links(&[]);
    let by = |fold: bool| {
        super::folders::find_folded(
            &orgs,
            Path::new("/srv/x/app"),
            Path::new("/Users/me"),
            &none,
            fold,
        )
        .organization
    };
    assert_eq!(by(true), "acme");
    assert_eq!(by(false), PERSONAL_ID);
}

#[test]
fn windows_folders_expand_standardize_and_match_without_host_path_rules() {
    let home = Path::new(r"C:\Users\me");
    assert_eq!(expand("~", home), Some(PathBuf::from(r"C:\Users\me")));
    assert_eq!(
        expand(r"~\work\acme\", home),
        Some(PathBuf::from(r"C:\Users\me\work\acme"))
    );
    assert_eq!(
        expand("c:/Work/./Acme/", home),
        Some(PathBuf::from(r"C:\Work\Acme"))
    );
    assert_eq!(
        expand(r"\\srv\share\proj\", home),
        Some(PathBuf::from(r"\\srv\share\proj"))
    );
    assert_eq!(
        standardize(Path::new(r"\\?\C:\Users\me\work\..\proj\")),
        Some(PathBuf::from(r"C:\Users\me\proj"))
    );
    assert!(super::folders::within(
        Path::new(r"c:\a\b"),
        Path::new(r"C:\a"),
        false
    ));
    assert!(!super::folders::within(
        Path::new(r"C:\ab"),
        Path::new(r"C:\a"),
        true
    ));

    let orgs = [
        org(PERSONAL_ID, &[]),
        org("home-org", &[r"~\work\aptus"]),
        org("drive-org", &[r"C:\srv\acme"]),
        org("unc-org", &[r"\\fileserver\shared\proj"]),
    ];
    let none = links(&[]);
    let by = |folder: &str, fold: bool| {
        let m = super::folders::find_folded(&orgs, Path::new(folder), home, &none, fold);
        m.organization
    };
    assert_eq!(by(r"c:\Users\me\work\aptus\fe", false), "home-org");
    assert_eq!(by(r"c:\users\me\WORK\aptus\fe", true), "home-org");
    assert_eq!(by("C:/srv/acme/app", false), "drive-org");
    assert_eq!(by(r"C:\srv\acme2", true), PERSONAL_ID);
    assert_eq!(by(r"\\FILESERVER\Shared\proj\sub", false), "unc-org");
}
