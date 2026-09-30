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
    /// the signed-in account's view (None while starting up)
    pub scope: Option<crate::accounts::Scope>,
}

/// Folders first, then names in alphabetical order (ignoring case).
fn sort_listing(v: &mut Vec<(String, bool, usize)>) {
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
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
        let mut v = Vfs { root: Node::dir(""), vol: open_volume(), persistent: false, scope: None };
        if !v.vol.is_null() {
            // Make sure \HYDATEK exists and is writable.
            if let Some(h) = open(v.vol, "\\HYDATEK", true, true) {
                close(h);
                v.persistent = true;
                v.root = load_dir(v.vol, "\\HYDATEK", "", 0);
            }
        }
        v.seed();
        v.seed_slides();
        v
    }

    /// The Hyda Slides sample (also added once to disks made before it existed).
    fn seed_slides(&mut self) {
        const MARK: &str = "/system/slides-sample-2";
        if self.exists(MARK) || self.get("/home").is_none() {
            return;
        }
        let pic = dunes_png();
        self.mkdir("/home/Documents/Presentations");
        if !self.exists("/home/Pictures/Dunes at dusk.png") {
            self.write("/home/Pictures/Dunes at dusk.png", &pic);
        }
        let path = "/home/Documents/Presentations/Meet HydatekOS.hydp";
        if !self.exists(path) {
            self.write(path, crate::deckio::to_hydp(&sample_deck(pic)).as_bytes());
        }
        self.write(MARK, b"1");
    }

    fn seed(&mut self) {
        const SCRIPTS_WELCOME: &str = "# Welcome to Hyda Scripts\n\
The **word processor** of *Hyda Workspace*, written from scratch for HydatekOS. Documents are saved in its own format, **.hyds**.\n\
## Try it\n\
- Pick a paragraph style from the menu at the left of the toolbar\n\
- Select text and press **B**, *I*, U or ~~S~~ (or Gen+B, Gen+I, Gen+U)\n\
- Centre or right-align paragraphs, and make bulleted or numbered lists\n\
## Sharing\n\
1. File › Export makes a Word (.docx), text or Markdown copy to send to others\n\
2. Word, text and Markdown files open here too; saving one creates a .hyds copy and leaves the original as it was\n\
## Keyboard\n\
- Gen+S saves, Gen+O opens, Gen+N starts a new document\n\
- Gen+Z undoes and Gen+Y redoes; Gen+X, Gen+C and Gen+V cut, copy and paste\n\
> Tip: click the document name at the top to rename it.\n";
        if self.get("/home").is_some() {
            return;
        }
        let welcome = b"Welcome to HydatekOS!\n\nThis is Notes. Everything you type here is saved to your\ndisk when you press Gen+S or click Save.\n\nTips\n- Click the grid button in the dock to see every app.\n- Drag windows by their title bar. Double-click to maximise.\n- Open Phone Link to pair HydatekOS Mobile.\n";
        let strategy = b"Hydatek Strategy 2026\n\n1. Ship HydatekOS milestone 1 (desktop + mobile shells).\n2. Own the stack: kernel, drivers, UI toolkit, apps.\n3. Phone Link: messages, notifications and files across devices.\n";
        let meeting = b"Meeting notes - 27 September\n\n- Review dock icons\n- Dark mode polish\n- Phone Link pairing flow\n";
        for d in ["/home", "/home/Documents", "/home/Pictures", "/home/Downloads", "/home/Shared", "/trash", "/system"] {
            self.mkdir(d);
        }
        for d in ["School projects", "Invoices", "Photos 2026", "Presentations"] {
            self.mkdir(&join("/home/Documents", d));
        }
        self.write("/home/Documents/Strategy.doc", strategy);
        let scripts = crate::doc::Doc::from_markdown(SCRIPTS_WELCOME).to_hyds();
        self.write("/home/Documents/Welcome to Hyda Scripts.hyds", scripts.as_bytes());
        self.write("/home/Documents/Budget.hydg", crate::gridio::to_hydg(&sample_budget()).as_bytes());
        self.write("/home/Documents/Meeting notes.txt", meeting);
        self.write("/home/Documents/Logo draft.img", b"HYDATEK-IMAGE");
        self.write("/home/Welcome.txt", welcome);
        self.write("/home/Pictures/Dunes.img", b"HYDATEK-IMAGE");
        self.write("/home/Pictures/Sunrise.img", b"HYDATEK-IMAGE");
        self.write("/home/Documents/School projects/Essay.txt", b"The desert at dawn...\n");
        self.write("/home/Documents/Invoices/INV-0042.txt", b"Invoice 0042\nTotal: 480.00\n");
    }

    pub fn get_raw(&self, path: &str) -> Option<&Node> {
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

    pub fn exists_raw(&self, path: &str) -> bool {
        self.get_raw(path).is_some()
    }

    pub fn is_dir_raw(&self, path: &str) -> bool {
        self.get_raw(path).map(|n| n.dir).unwrap_or(false)
    }

    pub fn list_raw(&self, path: &str) -> Vec<(String, bool, usize)> {
        let mut v: Vec<(String, bool, usize)> = match self.get_raw(path) {
            Some(n) => n.children.iter().filter(|c| !c.name.starts_with('.')).map(|c| (c.name.clone(), c.dir, c.size())).collect(),
            None => vec![],
        };
        sort_listing(&mut v);
        v
    }

    pub fn read_raw(&self, path: &str) -> Option<Vec<u8>> {
        self.get_raw(path).filter(|n| !n.dir).map(|n| n.data.clone())
    }

    pub fn mkdir_raw(&mut self, path: &str) -> bool {
        let parts = split(path);
        let mut cur = String::new();
        for p in parts {
            let par = if cur.is_empty() { String::from("/") } else { cur.clone() };
            cur.push('/');
            cur.push_str(p);
            if self.get_raw(&cur).is_none() {
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

    pub fn write_raw(&mut self, path: &str, data: &[u8]) -> bool {
        let dir = parent(path);
        let name = basename(path).to_string();
        if !self.mkdir_raw(&dir) {
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

    pub fn remove_raw(&mut self, path: &str) -> bool {
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
    pub fn rename_raw(&mut self, from: &str, to: &str) -> bool {
        if self.exists_raw(to) || !self.exists_raw(from) {
            return false;
        }
        fn copy(v: &mut Vfs, from: &str, to: &str) {
            let (dir, data, kids) = {
                let n = v.get_raw(from).unwrap();
                (n.dir, n.data.clone(), n.children.iter().map(|c| c.name.clone()).collect::<Vec<_>>())
            };
            if dir {
                v.mkdir_raw(to);
                for k in kids {
                    copy(v, &join(from, &k), &join(to, &k));
                }
            } else {
                v.write_raw(to, &data);
            }
        }
        copy(self, from, to);
        self.remove_raw(from)
    }

    // ---- the signed-in session's view ------------------------------------------
    //
    // Apps use these: `/home` and `/trash` are the account's own, `/home/Shared`
    // is everyone's, and other accounts' folders and the system folder can't
    // be reached (see accounts::map). The `*_raw` versions work on the tree as
    // stored, for the system itself.

    /// Where `path` is kept, if this session may reach it.
    fn phys(&self, path: &str) -> Option<String> {
        match &self.scope {
            None => Some(path.to_string()),
            Some(sc) => crate::accounts::map(sc, path),
        }
    }

    /// A folder every session has (it can't be moved or removed).
    fn fixed(&self, path: &str) -> bool {
        self.scope.is_some() && crate::accounts::is_fixed(path)
    }

    pub fn get(&self, path: &str) -> Option<&Node> {
        self.get_raw(&self.phys(path)?)
    }

    pub fn exists(&self, path: &str) -> bool {
        self.phys(path).map_or(false, |p| self.exists_raw(&p))
    }

    pub fn is_dir(&self, path: &str) -> bool {
        self.phys(path).map_or(false, |p| self.is_dir_raw(&p))
    }

    pub fn list(&self, path: &str) -> Vec<(String, bool, usize)> {
        let Some(p) = self.phys(path) else { return vec![] };
        let mut v = self.list_raw(&p);
        if let Some(sc) = &self.scope {
            if p == "/" {
                v.retain(|e| !crate::accounts::hidden_at_root(&e.0));
            }
            // the shared folder shows in every home
            if p == sc.home && sc.home != "/home" && !v.iter().any(|e| e.0 == "Shared") {
                if let Some(n) = self.get_raw(crate::accounts::SHARED) {
                    v.push((String::from("Shared"), true, n.size()));
                    sort_listing(&mut v);
                }
            }
        }
        v
    }

    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.read_raw(&self.phys(path)?)
    }

    pub fn mkdir(&mut self, path: &str) -> bool {
        match self.phys(path) {
            Some(p) => self.mkdir_raw(&p),
            None => false,
        }
    }

    pub fn write(&mut self, path: &str, data: &[u8]) -> bool {
        match self.phys(path) {
            Some(p) if !self.fixed(path) => self.write_raw(&p, data),
            _ => false,
        }
    }

    pub fn remove(&mut self, path: &str) -> bool {
        match self.phys(path) {
            Some(p) if !self.fixed(path) => self.remove_raw(&p),
            _ => false,
        }
    }

    pub fn rename(&mut self, from: &str, to: &str) -> bool {
        match (self.phys(from), self.phys(to)) {
            (Some(f), Some(t)) if !self.fixed(from) && !self.fixed(to) => self.rename_raw(&f, &t),
            _ => false,
        }
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

/// A whole file from the volume, however big (up to `max` bytes).
fn read_file_max(vol: *mut File, path: &str, max: usize) -> Option<Vec<u8>> {
    let h = open(vol, path, false, false)?;
    let mut out = vec![];
    let mut chunk = vec![0u8; 1 << 20];
    let mut ok = true;
    loop {
        let mut n = chunk.len();
        if unsafe { ((*h).read)(h, &mut n, chunk.as_mut_ptr()) } != efi::SUCCESS {
            ok = false;
            break;
        }
        if n == 0 {
            break;
        }
        out.extend_from_slice(&chunk[..n]);
        if out.len() > max {
            ok = false;
            break;
        }
    }
    close(h);
    ok.then_some(out)
}

impl Vfs {
    /// A file on the boot volume by its own path (`\EFI\BOOT\BOOTX64.EFI`),
    /// read whole: for the installer.
    pub fn read_volume(&self, path: &str, max: usize) -> Option<Vec<u8>> {
        if self.vol.is_null() {
            return None;
        }
        read_file_max(self.vol, path, max)
    }

    /// Everything under a folder of the boot volume, read whole: (path from
    /// that folder with '/' between names, bytes or None for a folder).
    /// None if it can't all be read or comes to more than `max` bytes.
    pub fn read_volume_tree(&self, dir: &str, max: usize) -> Option<Vec<(String, Option<Vec<u8>>)>> {
        if self.vol.is_null() {
            return None;
        }
        let mut out = Vec::new();
        let mut total = 0usize;
        let mut stack = vec![(String::from(dir), String::new(), 0u32)];
        while let Some((native, rel, depth)) = stack.pop() {
            if depth > 12 {
                continue;
            }
            for (child, is_dir) in read_entries(self.vol, &native) {
                let np = alloc::format!("{}\\{}", native, child);
                let rp = if rel.is_empty() { child.clone() } else { alloc::format!("{}/{}", rel, child) };
                if is_dir {
                    out.push((rp.clone(), None));
                    stack.push((np, rp, depth + 1));
                } else {
                    let data = read_file_max(self.vol, &np, max.saturating_sub(total))?;
                    total += data.len();
                    out.push((rp, Some(data)));
                }
            }
        }
        Some(out)
    }
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

/// A picture for the samples: dunes under an evening sky.
fn dunes_png() -> Vec<u8> {
    use crate::gfx::sin_q14;
    let (w, h) = (640i32, 360i32);
    let mut px = alloc::vec![0u32; (w * h) as usize];
    let mix = |a: u32, b: u32, t: i32| -> u32 {
        let t = t.clamp(0, 256) as u32;
        let f = |s: u32| ((((a >> s) & 255) * (256 - t) + ((b >> s) & 255) * t) >> 8) << s;
        0xFF00_0000 | f(16) | f(8) | f(0)
    };
    let dunes = [(250, 22, 3, 100, 0xC97B45u32), (285, 26, 2, 400, 0xA65A33), (320, 18, 4, 700, 0x6E3A26)];
    for y in 0..h {
        for x in 0..w {
            // sky: violet to amber towards the horizon
            let mut c = mix(0x2B2045, 0xF0A868, y * 256 / 260);
            // the sun and its glow
            let (dx, dy) = (x - 430, y - 205);
            let d2 = dx * dx + dy * dy;
            if d2 < 120 * 120 {
                c = mix(c, 0xF8D58C, ((120 * 120 - d2) * 90 / (120 * 120 - 58 * 58)).min(90));
            }
            if d2 < 59 * 59 {
                c = mix(c, 0xF8D58C, ((59 * 59 - d2) * 256 / (59 * 59 - 57 * 57)).min(256));
            }
            for &(base, amp, freq, phase, col) in &dunes {
                let a = (x * freq * 1024 / w + phase) & 1023;
                // the crest in 1/256 px, so its edge can be smoothed
                let top = base * 256 + amp * sin_q14(a) / 64;
                let cover = (y * 256 + 256 - top).clamp(0, 256);
                if cover > 0 {
                    // lighter along the crest
                    let lit = mix(col, 0xFFE0B0, (12 * 256 - (y * 256 - top).max(0)).max(0) * 6 / 256);
                    c = mix(c, lit, cover);
                }
            }
            px[(y * w + x) as usize] = c;
        }
    }
    crate::deckio::png_encode(w as u32, h as u32, &px)
}

/// The sample presentation for Hyda Slides.
fn sample_deck(pic: Vec<u8>) -> crate::deck::Deck {
    use crate::deck::*;
    use crate::doc::{Pos, Style, BOLD};
    let mut d = Deck::new();
    d.name = String::from("Meet HydatekOS");
    d.footer = String::from("Meet HydatekOS · Lagos 2026");
    d.numbers = true;
    d.slides.clear();
    let bullets = |sh: &mut Shape, items: &[(&str, u8)]| {
        let text: Vec<&str> = items.iter().map(|x| x.0).collect();
        sh.set_plain(&text.join("\n"));
        for (p, (_, lvl)) in sh.text.paras.iter_mut().zip(items.iter()) {
            p.style = Style::Bullet;
            p.level = *lvl;
        }
    };
    let mut s = d.new_slide(Layout::Title);
    s.shapes[0].set_plain("Meet HydatekOS");
    s.shapes[1].set_plain("An operating system built from scratch · Lagos, 2026");
    s.notes = String::from("Welcome everyone.\nEverything you will see today runs on HydatekOS, and this deck was made in Hyda Slides.");
    s.trans = Trans::Fade;
    d.slides.push(s);
    let mut s = d.new_slide(Layout::TitleContent);
    s.shapes[0].set_plain("What's inside");
    bullets(&mut s.shapes[1], &[("A desktop and a phone shell", 0), ("Hyda Workspace", 0), ("Scripts, Grids and Slides", 1), ("Documents, sheets and presentations", 1), ("A browser with its own search engine", 0), ("Phone Link to your phone", 0)]);
    s.shapes[1].text.set_fmt(Pos::new(1, 0), Pos::new(1, 14), BOLD, true);
    s.notes = String::from("One line per item; details come later.");
    s.trans = Trans::Fade;
    d.slides.push(s);
    let mut s = d.new_slide(Layout::TwoContent);
    s.shapes[0].set_plain("Built from scratch");
    bullets(&mut s.shapes[1], &[("Kernel and drivers", 0), ("Graphics, fonts and icons", 0), ("TCP/IP and TLS", 0)]);
    bullets(&mut s.shapes[2], &[("Picture decoders", 0), ("Office file formats", 0), ("Every app you see", 0)]);
    s.trans = Trans::Fade;
    d.slides.push(s);
    let mut s = d.new_slide(Layout::TitleOnly);
    s.shapes[0].set_plain("Pictures, shapes and text");
    d.pics.push(Pic { data: alloc::rc::Rc::new(pic) });
    let mut p = Shape::new(Kind::Picture, 80, 180, 720, 405);
    p.pic = Some(0);
    s.shapes.push(p);
    let mut r = Shape::new(Kind::Rect, 840, 200, 360, 170);
    r.set_plain("Dunes at dusk");
    r.size = 28;
    s.shapes.push(r);
    let mut e = Shape::new(Kind::Ellipse, 930, 410, 180, 180);
    e.fill = Some(0xF2B544);
    e.set_plain("New");
    e.size = 24;
    e.color = Some(0x1E1B2C);
    s.shapes.push(e);
    s.trans = Trans::Push;
    d.slides.push(s);
    // charts and tables
    let mut s = d.new_slide(Layout::TitleOnly);
    s.shapes[0].set_plain("By the numbers");
    let mut ch = Shape::new(Kind::Chart, 70, 170, 640, 470);
    ch.chart = Some(Chart {
        kind: ChartKind::Column,
        title: String::from("Active devices (thousands)"),
        cats: ["Q1", "Q2", "Q3", "Q4"].iter().map(|c| String::from(*c)).collect(),
        series: alloc::vec![Series { name: String::from("2025"), vals: alloc::vec![12.0, 18.5, 24.0, 31.0] }, Series { name: String::from("2026"), vals: alloc::vec![28.0, 39.5, 47.0, 62.5] }],
        legend: true,
    });
    s.shapes.push(ch);
    let mut tb = Shape::new(Kind::Table, 750, 200, 460, 200);
    let mut t = Table::new(4, 2, 460, 200);
    for (i, (a, b)) in [("App", "Daily users"), ("Browser", "41,200"), ("Hyda Workspace", "18,900"), ("Phone Link", "12,300")].iter().enumerate() {
        t.cell_mut(i, 0).insert(Pos::new(0, 0), a, 0);
        t.cell_mut(i, 1).insert(Pos::new(0, 0), b, 0);
        t.cell_mut(i, 1).paras[0].align = crate::deck::Align::Right;
    }
    tb.table = Some(t);
    tb.fit_table();
    s.shapes.push(tb);
    let mut pie = Shape::new(Kind::Chart, 780, 420, 400, 250);
    pie.chart = Some(Chart::sample(ChartKind::Pie));
    pie.chart.as_mut().unwrap().legend = false;
    s.shapes.push(pie);
    s.trans = Trans::Fade;
    d.slides.push(s);
    // shapes, arrows and animations
    let mut s = d.new_slide(Layout::TitleOnly);
    s.shapes[0].set_plain("How a presentation is made");
    let steps = ["Write", "Design", "Rehearse"];
    for (k, name) in steps.iter().enumerate() {
        let mut c = Shape::new(Kind::Rect, 90 + k as i32 * 330, 250, 280, 130);
        c.geom = Geom::Chevron;
        c.fill = Some([0xC0622B, 0xD9A441, 0x0E5A43][k]);
        c.set_plain(name);
        c.size = 26;
        c.anim = Anim::Fly;
        c.anim_order = k as u16 + 1;
        s.shapes.push(c);
    }
    let mut star = Shape::new(Kind::Rect, 1060, 430, 150, 150);
    star.geom = Geom::Star5;
    star.fill = Some(0xF2B544);
    star.rot = 12;
    star.anim = Anim::Fade;
    star.anim_order = 4;
    s.shapes.push(star);
    let mut g = Shape::new(Kind::Rect, 90, 450, 560, 110);
    g.geom = Geom::RoundRect;
    g.fill = Some(0x2F6FEB);
    g.grad = Some((0x8E6CB5, 0));
    g.set_plain("Then press F5 to present");
    g.size = 24;
    g.anim = Anim::Fade;
    g.anim_order = 5;
    s.shapes.push(g);
    let mut ln = Shape::new(Kind::Line, 0, 0, 0, 0);
    set_line_ends(&mut ln, (670, 505), (1040, 505));
    ln.line = Some(0x1E1B2C);
    ln.line_w = 4;
    ln.tail = true;
    s.shapes.push(ln);
    s.notes = String::from("Each click brings in the next step.");
    s.trans = Trans::Push;
    d.slides.push(s);
    let mut s = d.new_slide(Layout::Section);
    s.shapes[0].set_plain("Thank you");
    s.shapes[1].set_plain("Press Esc to leave the slideshow");
    s.trans = Trans::Fade;
    d.slides.push(s);
    d
}

/// The starter sheet for Hyda Grids.
fn sample_budget() -> crate::grid::Sheet {
    use crate::grid::{Fmt, HAlign, Num, Sheet};
    let mut s = Sheet::new();
    s.name = String::from("Budget");
    let rows: [[&str; 4]; 7] = [
        ["Item", "Q3", "Q4", "Total"],
        ["Hardware", "12000", "14000", "=B2+C2"],
        ["Design", "6000", "6500", "=B3+C3"],
        ["Cloud", "3000", "3200", "=B4+C4"],
        ["Marketing", "4500", "5200", "=B5+C5"],
        ["Total", "=SUM(B2:B5)", "=SUM(C2:C5)", "=SUM(D2:D5)"],
        ["Growth", "", "=C6/B6-1", ""],
    ];
    for (r, row) in rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            s.set_input(r as u32, c as u32, v);
        }
    }
    let money = Fmt { num: Num::Currency, sym: '\u{20a6}', ..Fmt::default() };
    for r in 1..6 {
        for c in 1..4 {
            s.set_fmt(r, c, Fmt { bold: r == 5, ..money });
        }
    }
    for c in 0..4 {
        s.set_fmt(0, c, Fmt { bold: true, align: if c == 0 { HAlign::Auto } else { HAlign::Right }, ..Fmt::default() });
    }
    s.set_fmt(5, 0, Fmt { bold: true, ..Fmt::default() });
    s.set_fmt(6, 0, Fmt { italic: true, ..Fmt::default() });
    s.set_fmt(6, 2, Fmt { num: Num::Percent, italic: true, ..Fmt::default() });
    s.widths.insert(0, 140);
    s.widths.insert(3, 120);
    s
}
