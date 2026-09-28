//! Accounts: the people who use this computer, and where each one's things
//! are kept. One person is signed in at a time; their session sees their own
//! home folder and Bin as `/home` and `/trash`, and `/home/Shared` is common
//! to everyone.
//!
//! `/system/users.txt` lists the accounts:
//!
//! ```text
//! user u1 admin                the first account (made by the setup assistant)
//! user u2 standard new         "new": hasn't been through the setup assistant
//! last u1                      who signed in last
//! ```
//!
//! The first account keeps the places HydatekOS always used (`/home`, `/trash`,
//! `/system/profile.txt`...), so a computer set up before accounts existed
//! carries on as account `u1`. Other accounts live under `/users/<id>/` (files)
//! and `/system/users/<id>/` (profile, sign-in, settings, calendar).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// The first account's id.
pub const FIRST: &str = "u1";
/// Where the folder shared by every account is kept.
pub const SHARED: &str = "/home/Shared";
/// Most accounts on one computer.
pub const MAX: usize = 8;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Account {
    /// "u1", "u2"...: letters and digits only (it names folders)
    pub id: String,
    /// may add and remove accounts
    pub admin: bool,
    /// made by someone else and not set up yet by its owner
    pub new: bool,
}

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Accounts {
    pub list: Vec<Account>,
    /// who signed in last
    pub last: String,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 16 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

impl Accounts {
    /// Read the list; bad or repeated lines are skipped.
    pub fn parse(text: &str) -> Accounts {
        let mut a = Accounts::default();
        for line in text.lines() {
            let mut it = line.split_whitespace();
            match it.next() {
                Some("user") => {
                    let Some(id) = it.next().filter(|id| valid_id(id)) else { continue };
                    if a.get(id).is_some() || a.list.len() >= MAX {
                        continue;
                    }
                    let flags: Vec<&str> = it.collect();
                    a.list.push(Account { id: String::from(id), admin: flags.contains(&"admin"), new: flags.contains(&"new") });
                }
                Some("last") => a.last = String::from(it.next().unwrap_or("")),
                _ => {}
            }
        }
        if a.get(&a.last.clone()).is_none() {
            a.last = a.list.first().map(|x| x.id.clone()).unwrap_or_default();
        }
        // someone must be able to manage accounts
        if !a.list.is_empty() && a.admins() == 0 {
            a.list[0].admin = true;
        }
        a
    }

    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for u in &self.list {
            s.push_str(&format!("user {} {}{}\n", u.id, if u.admin { "admin" } else { "standard" }, if u.new { " new" } else { "" }));
        }
        if !self.last.is_empty() {
            s.push_str(&format!("last {}\n", self.last));
        }
        s
    }

    pub fn get(&self, id: &str) -> Option<&Account> {
        self.list.iter().find(|u| u.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Account> {
        self.list.iter_mut().find(|u| u.id == id)
    }

    pub fn admins(&self) -> usize {
        self.list.iter().filter(|u| u.admin).count()
    }

    /// The smallest unused id: "u1", "u2"...
    pub fn new_id(&self) -> String {
        (1..).map(|n| format!("u{}", n)).find(|id| self.get(id).is_none()).unwrap()
    }

    /// Can `id` be removed? Not the first account (it holds the computer's
    /// original folders), not the last administrator.
    pub fn removable(&self, id: &str) -> bool {
        match self.get(id) {
            Some(u) => u.id != FIRST && !(u.admin && self.admins() == 1),
            None => false,
        }
    }

    /// Can `id` stop being an administrator?
    pub fn demotable(&self, id: &str) -> bool {
        self.get(id).map_or(false, |u| u.admin && self.admins() > 1)
    }
}

/// An account's home folder, Bin and system folder, as stored.
pub fn home_dir(id: &str) -> String {
    if id == FIRST { String::from("/home") } else { format!("/users/{}/home", id) }
}

pub fn trash_dir(id: &str) -> String {
    if id == FIRST { String::from("/trash") } else { format!("/users/{}/trash", id) }
}

pub fn system_dir(id: &str) -> String {
    if id == FIRST { String::from("/system") } else { format!("/system/users/{}", id) }
}

/// Everything an account keeps, for removing it (its files and settings).
pub fn account_dirs(id: &str) -> Vec<String> {
    if id == FIRST {
        return Vec::new();
    }
    alloc::vec![format!("/users/{}", id), format!("/system/users/{}", id)]
}

/// What a signed-in session sees.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Scope {
    pub home: String,
    pub trash: String,
}

impl Scope {
    pub fn of(id: &str) -> Scope {
        Scope { home: home_dir(id), trash: trash_dir(id) }
    }
}

/// Where a path a session uses is kept, or `None` if the session may not
/// reach it: other accounts' folders (`/users`), the system folder (profiles,
/// sign-in hashes, settings) and anything with `..` in it.
///
/// `/home/...` is the account's home, `/home/Shared/...` the shared folder and
/// `/trash/...` its Bin; other paths are left as they are.
pub fn map(scope: &Scope, path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    if parts.iter().any(|p| *p == "..") {
        return None;
    }
    let rest = |from: usize| -> String { parts[from..].iter().map(|p| format!("/{}", p)).collect() };
    match parts.first().copied() {
        None => Some(String::from("/")),
        Some("home") if parts.get(1) == Some(&"Shared") => Some(format!("{}{}", SHARED, rest(2))),
        Some("home") => Some(format!("{}{}", scope.home, rest(1))),
        Some("trash") => Some(format!("{}{}", scope.trash, rest(1))),
        Some("system") | Some("users") => None,
        Some(_) => Some(rest(0)),
    }
}

/// The folders every session has, which can't be moved or removed.
pub fn is_fixed(path: &str) -> bool {
    let p = path.trim_end_matches('/');
    matches!(p, "" | "/home" | "/trash" | "/home/Shared")
}

/// Names hidden from a session's view of the top folder.
pub fn hidden_at_root(name: &str) -> bool {
    name == "system" || name == "users"
}
