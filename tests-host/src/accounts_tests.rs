//! Accounts: the list's file, ids, who may be removed, and the path rules
//! that keep each person's files to themselves.

use crate::accounts::*;

fn two() -> Accounts {
    Accounts::parse("user u1 admin\nuser u2 standard new\nlast u2\n")
}

#[test]
fn list_round_trip() {
    let a = two();
    assert_eq!(a.list.len(), 2);
    assert!(a.list[0].admin && !a.list[0].new);
    assert!(!a.list[1].admin && a.list[1].new);
    assert_eq!(a.last, "u2");
    assert_eq!(a.to_text(), "user u1 admin\nuser u2 standard new\nlast u2\n");
    assert_eq!(Accounts::parse(&a.to_text()), a);
    assert_eq!(Accounts::parse(""), Accounts::default());
}

#[test]
fn damaged_lists() {
    // bad ids (path tricks, capitals), repeats and junk are skipped
    let a = Accounts::parse("user ../x admin\nuser U3 admin\nuser u1 standard\nuser u1 admin\nuser u2 standard\nnonsense\nlast u9\n");
    assert_eq!(a.list.iter().map(|u| u.id.as_str()).collect::<Vec<_>>(), ["u1", "u2"]);
    // someone who left: "last" falls back to the first account
    assert_eq!(a.last, "u1");
    // nobody an administrator: the first becomes one
    assert!(a.list[0].admin && !a.list[1].admin);
    // no more than MAX accounts
    let many: String = (1..=12).map(|i| format!("user u{} admin\n", i)).collect();
    assert_eq!(Accounts::parse(&many).list.len(), MAX);
}

#[test]
fn ids_and_rules() {
    let mut a = two();
    assert_eq!(a.new_id(), "u3");
    a.list.remove(0);
    assert_eq!(a.new_id(), "u1");
    let a = Accounts::parse("user u1 admin\nuser u2 standard\nuser u3 admin\n");
    // the first account stays; others may go
    assert!(!a.removable("u1"));
    assert!(a.removable("u2") && a.removable("u3"));
    assert!(!a.removable("u9"));
    // the last administrator can't go or stop being one
    let b = Accounts::parse("user u1 standard\nuser u2 admin\n");
    assert!(!b.removable("u2") && !b.demotable("u2"));
    assert!(a.demotable("u1") && a.demotable("u3") && !a.demotable("u2"));
    assert_eq!(a.admins(), 2);
}

#[test]
fn where_things_are_kept() {
    // the first account keeps the original places
    assert_eq!((home_dir("u1"), trash_dir("u1"), system_dir("u1")), ("/home".into(), "/trash".into(), "/system".into()));
    assert_eq!((home_dir("u2"), trash_dir("u2"), system_dir("u2")), ("/users/u2/home".into(), "/users/u2/trash".into(), "/system/users/u2".into()));
    assert!(account_dirs("u1").is_empty());
    assert_eq!(account_dirs("u2"), vec![String::from("/users/u2"), String::from("/system/users/u2")]);
}

#[test]
fn session_paths() {
    let first = Scope::of("u1");
    let tunde = Scope::of("u2");
    let m = |s: &Scope, p: &str| map(s, p);
    // your home and Bin
    assert_eq!(m(&tunde, "/home"), Some("/users/u2/home".into()));
    assert_eq!(m(&tunde, "/home/Documents/Essay.txt"), Some("/users/u2/home/Documents/Essay.txt".into()));
    assert_eq!(m(&tunde, "/trash/old.txt"), Some("/users/u2/trash/old.txt".into()));
    assert_eq!(m(&first, "/home/Documents"), Some("/home/Documents".into()));
    assert_eq!(m(&first, "/trash"), Some("/trash".into()));
    // Shared is the same folder for everyone
    assert_eq!(m(&tunde, "/home/Shared/plan.txt"), Some("/home/Shared/plan.txt".into()));
    assert_eq!(m(&first, "/home/Shared/plan.txt"), Some("/home/Shared/plan.txt".into()));
    // other accounts and the system folder are out of reach
    for p in ["/users", "/users/u1/home/x", "/system", "/system/lock.txt", "/system/users/u1/profile.txt", "/home/../system/lock.txt", "/home/Documents/../../users/u1"] {
        assert_eq!(m(&tunde, p), None, "{}", p);
        assert_eq!(m(&first, p), None, "{}", p);
    }
    // the first account can't reach another's files through /home either
    assert_eq!(m(&first, "/home/../users/u2/home"), None);
    // other places, tidy paths
    assert_eq!(m(&tunde, "/"), Some("/".into()));
    assert_eq!(m(&tunde, "/apps/hydatek-link.apk"), Some("/apps/hydatek-link.apk".into()));
    assert_eq!(m(&tunde, "//home/./Documents/"), Some("/users/u2/home/Documents".into()));
    // the fixed folders
    for p in ["/home", "/home/", "/trash", "/home/Shared"] {
        assert!(is_fixed(p), "{}", p);
    }
    assert!(!is_fixed("/home/Documents") && !is_fixed("/home/Shared/x"));
    assert!(hidden_at_root("system") && hidden_at_root("users") && !hidden_at_root("home"));
}
