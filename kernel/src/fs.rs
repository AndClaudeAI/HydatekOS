//! HydatekOS virtual file system.
//!
//! The tree lives in memory and is written through to the boot volume under
//! `\HYDATEK\` so user files survive reboots on real hardware. If the boot
//! volume is read-only (e.g. a CD image), HydatekOS runs as a live session.

use crate::efi::{self, File, SimpleFs, LoadedImage};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::ptr::null_mut;

pub struct Node {
    pub name: String,
    pub dir: bool,
    pub data: Vec<u8>,
    pub children: Vec<Node>,
}

impl Node {
    fn dir(name: &str) -> Node {
        Node { name: name.to_string(), dir: true, data: vec![], children: vec![] }
    }
    fn file(name: &str, data: &[u8]) -> Node {
        Node { name: name.to_string(), dir: false, data: data.to_vec(), children: vec![] }
    }
    pub fn size(&self) -> usize {
        if self.dir { self.children.iter().map(|c| c.size()).sum() } else { self.data.len() }
    }
}

pub struct Vfs {
    pub root: Node,
    vol: *mut File,
    pub persistent: bool,
}

pub fn split(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

pub fn join(dir: &str, name: &str) -> String {
    if dir == "/" { alloc::format!("/{}", name) } else { alloc::format!("{}/{}", dir, name) }
}

pub fn parent(path: &str) -> String {
    let mut p = split(path);
    p.pop();
    let mut s = String::from("/");
    s.push_str(&p.join("/"));
    s
}

pub fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl Vfs {
    pub fn mount() -> Vfs {
        let mut v = Vfs { root: Node::dir(""), vol: open_volume(), persistent: false };
        if !v.vol.is_null() {
            // Make sure \HYDATEK exists and is writable.
            if let Some(h) = open(v.vol, "\\HYDATEK", true, true) {
                close(h);
                v.persistent = true;
                v.root = load_dir(v.vol, "\\HYDATEK", "", 0);
            }
        }
        v.seed();
        v
    }

    fn seed(&mut self) {
        const SCRIPTS_WELCOME: &str = "# Welcome to Hyda Scripts\n\
The **word processor** of *Hyda Workspace*. Documents are saved as Word files (.docx), so they open in Microsoft Word, LibreOffice and Google Docs.\n\
## Try it\n\
- Pick a paragraph style from the menu at the left of the toolbar\n\
- Select text and press **B**, *I*, U or ~~S~~ (or Ctrl+B, Ctrl+I, Ctrl+U)\n\
- Centre or right-align paragraphs, and make bulleted or numbered lists\n\
## Keyboard\n\
1. Ctrl+S saves, Ctrl+O opens, Ctrl+N starts a new document\n\
2. Ctrl+Z undoes and Ctrl+Y redoes\n\
3. Ctrl+X, Ctrl+C and Ctrl+V cut, copy and paste\n\
> Tip: click the document name at the top to rename it.\n";
        if self.get("/home").is_some() {
            return;
        }
        let welcome = b"Welcome to HydatekOS!\n\nThis is Notes. Everything you type here is saved to your\ndisk when you press Ctrl+S or click Save.\n\nTips\n- Click the grid button in the dock to see every app.\n- Drag windows by their title bar. Double-click to maximise.\n- Open Phone Link to pair HydatekOS Mobile.\n";
        let strategy = b"Hydatek Strategy 2026\n\n1. Ship HydatekOS milestone 1 (desktop + mobile shells).\n2. Own the stack: kernel, drivers, UI toolkit, apps.\n3. Phone Link: messages, notifications and files across devices.\n";
        let budget = b"Item, Q3, Q4\nHardware, 12000, 14000\nDesign, 6000, 6500\nCloud, 3000, 3200\n";
        let meeting = b"Meeting notes - 27 September\n\n- Review dock icons\n- Dark mode polish\n- Phone Link pairing flow\n";
        for d in ["/home", "/home/Documents", "/home/Pictures", "/home/Downloads", "/home/Shared", "/trash", "/system"] {
            self.mkdir(d);
        }
        for d in ["School projects", "Invoices", "Photos 2026", "Presentations"] {
            self.mkdir(&join("/home/Documents", d));
        }
        self.write("/home/Documents/Strategy.doc", strategy);
        let scripts = crate::doc::Doc::from_markdown(SCRIPTS_WELCOME).to_docx();
        self.write("/home/Documents/Welcome to Hyda Scripts.docx", &scripts);
        self.write("/home/Documents/Budget.sheet", budget);
        self.write("/home/Documents/Meeting notes.txt", meeting);
        self.write("/home/Documents/Logo draft.img", b"HYDATEK-IMAGE");
        self.write("/home/Welcome.txt", welcome);
        self.write("/home/Pictures/Dunes.img", b"HYDATEK-IMAGE");
        self.write("/home/Pictures/Sunrise.img", b"HYDATEK-IMAGE");
        self.write("/home/Documents/School projects/Essay.txt", b"The desert at dawn...\n");
        self.write("/home/Documents/Invoices/INV-0042.txt", b"Invoice 0042\nTotal: 480.00\n");
    }

    pub fn get(&self, path: &str) -> Option<&Node> {
        let mut n = &self.root;
        for part in split(path) {
            n = n.children.iter().find(|c| c.name == part)?;
        }
        Some(n)
    }

    fn get_mut(&mut self, path: &str) -> Option<&mut Node> {
        let mut n = &mut self.root;
        for part in split(path) {
            n = n.children.iter_mut().find(|c| c.name == part)?;
        }
        Some(n)
    }

    pub fn exists(&self, path: &str) -> bool {
        self.get(path).is_some()
    }

    pub fn is_dir(&self, path: &str) -> bool {
        self.get(path).map(|n| n.dir).unwrap_or(false)
    }

    pub fn list(&self, path: &str) -> Vec<(String, bool, usize)> {
        let mut v: Vec<(String, bool, usize)> = match self.get(path) {
            Some(n) => n.children.iter().filter(|c| !c.name.starts_with('.')).map(|c| (c.name.clone(), c.dir, c.size())).collect(),
            None => vec![],
        };
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
        v
    }

    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.get(path).filter(|n| !n.dir).map(|n| n.data.clone())
    }

    pub fn mkdir(&mut self, path: &str) -> bool {
        let parts = split(path);
        let mut cur = String::new();
        for p in parts {
            let par = if cur.is_empty() { String::from("/") } else { cur.clone() };
            cur.push('/');
            cur.push_str(p);
            if self.get(&cur).is_none() {
                match self.get_mut(&par) {
                    Some(n) if n.dir => n.children.push(Node::dir(p)),
                    _ => return false,
                }
                if self.persistent {
                    if let Some(h) = open(self.vol, &native(&cur), true, true) {
                        close(h);
                    }
                }
            }
        }
        true
    }

    pub fn write(&mut self, path: &str, data: &[u8]) -> bool {
        let dir = parent(path);
        let name = basename(path).to_string();
        if !self.mkdir(&dir) {
            return false;
        }
        let d = self.get_mut(&dir).unwrap();
        match d.children.iter_mut().find(|c| c.name == name) {
            Some(n) if n.dir => return false,
            Some(n) => n.data = data.to_vec(),
            None => d.children.push(Node::file(&name, data)),
        }
        if self.persistent {
            write_native(self.vol, &native(path), data);
        }
        true
    }

    pub fn remove(&mut self, path: &str) -> bool {
        let dir = parent(path);
        let name = basename(path).to_string();
        if self.persistent {
            delete_native(self.vol, &native(path));
        }
        match self.get_mut(&dir) {
            Some(d) => {
                let before = d.children.len();
                d.children.retain(|c| c.name != name);
                before != d.children.len()
            }
            None => false,
        }
    }

    /// Move a file or folder (implemented as copy + delete for the disk backend).
    pub fn rename(&mut self, from: &str, to: &str) -> bool {
        if self.exists(to) || !self.exists(from) {
            return false;
        }
        fn copy(v: &mut Vfs, from: &str, to: &str) {
            let (dir, data, kids) = {
                let n = v.get(from).unwrap();
                (n.dir, n.data.clone(), n.children.iter().map(|c| c.name.clone()).collect::<Vec<_>>())
            };
            if dir {
                v.mkdir(to);
                for k in kids {
                    copy(v, &join(from, &k), &join(to, &k));
                }
            } else {
                v.write(to, &data);
            }
        }
        copy(self, from, to);
        self.remove(from)
    }

    /// A name in `dir` that does not exist yet ("Untitled 2.txt").
    pub fn unique(&self, dir: &str, stem: &str, ext: &str) -> String {
        for i in 1..1000 {
            let name = if i == 1 { alloc::format!("{}{}", stem, ext) } else { alloc::format!("{} {}{}", stem, i, ext) };
            if !self.exists(&join(dir, &name)) {
                return join(dir, &name);
            }
        }
        join(dir, stem)
    }
}

// ---------------------------------------------------------------------------
// UEFI Simple File System backend

fn native(path: &str) -> String {
    let mut s = String::from("\\HYDATEK");
    for p in split(path) {
        s.push('\\');
        s.push_str(p);
    }
    s
}

fn utf16(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

fn open_volume() -> *mut File {
    let li: *mut LoadedImage = match efi::handle_protocol(efi::image(), &efi::LOADED_IMAGE_GUID) {
        Some(p) => p,
        None => return null_mut(),
    };
    let dev = unsafe { (*li).device_handle };
    let sfs: *mut SimpleFs = match efi::handle_protocol(dev, &efi::SIMPLE_FS_GUID) {
        Some(p) => p,
        None => return null_mut(),
    };
    let mut root: *mut File = null_mut();
    if unsafe { ((*sfs).open_volume)(sfs, &mut root) } != efi::SUCCESS {
        return null_mut();
    }
    root
}

fn open(vol: *mut File, path: &str, create: bool, dir: bool) -> Option<*mut File> {
    let name = utf16(path);
    let mut h: *mut File = null_mut();
    let mode = efi::FILE_READ | efi::FILE_WRITE | if create { efi::FILE_CREATE } else { 0 };
    let attr = if dir && create { efi::ATTR_DIRECTORY } else { 0 };
    let s = unsafe { ((*vol).open)(vol, &mut h, name.as_ptr(), mode, attr) };
    if s == efi::SUCCESS && !h.is_null() { Some(h) } else { None }
}

fn close(h: *mut File) {
    unsafe { ((*h).close)(h) };
}

fn write_native(vol: *mut File, path: &str, data: &[u8]) {
    // Recreate the file so shorter contents truncate correctly.
    if let Some(h) = open(vol, path, false, false) {
        unsafe { ((*h).delete)(h) };
    }
    if let Some(h) = open(vol, path, true, false) {
        let mut n = data.len();
        unsafe {
            ((*h).write)(h, &mut n, data.as_ptr());
            ((*h).flush)(h);
        }
        close(h);
    }
}

fn delete_native(vol: *mut File, path: &str) {
    // Directories must be emptied first.
    for (name, is_dir) in read_entries(vol, path) {
        let child = alloc::format!("{}\\{}", path, name);
        if is_dir {
            delete_native(vol, &child);
        } else if let Some(h) = open(vol, &child, false, false) {
            unsafe { ((*h).delete)(h) };
        }
    }
    if let Some(h) = open(vol, path, false, false) {
        unsafe { ((*h).delete)(h) };
    }
}

fn read_entries(vol: *mut File, path: &str) -> Vec<(String, bool)> {
    let mut out = vec![];
    let h = match open(vol, path, false, false) {
        Some(h) => h,
        None => return out,
    };
    let mut buf = vec![0u8; 1024];
    loop {
        let mut n = buf.len();
        let s = unsafe { ((*h).read)(h, &mut n, buf.as_mut_ptr()) };
        if s != efi::SUCCESS || n == 0 {
            break;
        }
        let attr = u64::from_le_bytes(buf[72..80].try_into().unwrap());
        let mut name = String::new();
        let mut i = 80;
        while i + 1 < n {
            let c = u16::from_le_bytes([buf[i], buf[i + 1]]);
            if c == 0 {
                break;
            }
            name.push(char::from_u32(c as u32).unwrap_or('?'));
            i += 2;
        }
        if name != "." && name != ".." {
            out.push((name, attr & efi::ATTR_DIRECTORY != 0));
        }
    }
    close(h);
    out
}

fn read_file(vol: *mut File, path: &str) -> Vec<u8> {
    let mut out = vec![];
    if let Some(h) = open(vol, path, false, false) {
        let mut chunk = vec![0u8; 16384];
        loop {
            let mut n = chunk.len();
            if unsafe { ((*h).read)(h, &mut n, chunk.as_mut_ptr()) } != efi::SUCCESS || n == 0 {
                break;
            }
            out.extend_from_slice(&chunk[..n]);
            if out.len() > 4 << 20 {
                break;
            }
        }
        close(h);
    }
    out
}

fn load_dir(vol: *mut File, npath: &str, name: &str, depth: u32) -> Node {
    let mut n = Node::dir(name);
    if depth > 8 {
        return n;
    }
    for (child, is_dir) in read_entries(vol, npath) {
        let cp = alloc::format!("{}\\{}", npath, child);
        if is_dir {
            n.children.push(load_dir(vol, &cp, &child, depth + 1));
        } else {
            let data = read_file(vol, &cp);
            n.children.push(Node::file(&child, &data));
        }
    }
    n
}
