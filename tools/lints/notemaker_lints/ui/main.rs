#![allow(dead_code, unused_variables)]

pub struct PageNumber(u32);
pub struct Title(String);
pub struct Wrapper<T>(T);
pub enum LoadError {
    Missing,
}
type Alias = String;

// Flagged: primitives, std types, arrays, slices, tuples, aliases.
pub fn primitive(page: u32) -> char {
    'p'
}
pub fn bool_parameter(redo: bool) {}
pub fn wrapped_raw(pages: std::sync::Arc<std::sync::Mutex<Vec<u32>>>) {}
pub(crate) fn crate_visible(name: &str) {}
pub fn array(points: [PageNumber; 2]) {}
pub fn slice(pages: &[PageNumber]) {}
pub fn tuple() -> (PageNumber, Title) {
    unimplemented!()
}
pub fn alias(title: Alias) {}
pub fn vec(pages: Vec<PageNumber>) {}
pub fn inside_option(page: Option<u32>) {}
pub fn inside_result() -> Result<Title, String> {
    unimplemented!()
}
pub fn inside_local_generic(value: Wrapper<i64>) {}
pub fn fn_pointer(callback: fn(u8)) {}
pub extern "system" fn exported_jni(handle: i64) {}

mod nested {
    // `pub` inside a private module is still checked.
    pub fn nested_pub(count: usize) {}
    pub(super) fn super_visible(path: std::path::PathBuf) {}
    fn nested_private(count: usize) {}
}

pub struct Document;

impl Document {
    pub fn method(&self, page: u32) {}
    pub fn ok_method(&mut self, page: PageNumber) -> Option<Title> {
        None
    }
    fn private_method(&self, page: u32) {}
}

pub trait Renderer {
    fn render(&self, scale: f32);
}

trait PrivateTrait {
    fn private_trait(&self, scale: f32);
}

// Allowed.
pub fn newtypes(page: PageNumber, title: &Title) -> Result<Option<Title>, LoadError> {
    Ok(None)
}
pub fn unit() {}
pub fn predicate() -> bool {
    true
}
pub fn predicate_result() -> Result<bool, LoadError> {
    Ok(true)
}
pub fn wrappers(
    shared: std::sync::Arc<Document>,
    boxed: Box<Document>,
    local: std::rc::Rc<std::cell::RefCell<Document>>,
    guarded: &std::sync::Mutex<Option<Title>>,
    lock: std::sync::RwLock<Document>,
    once: &std::sync::OnceLock<Title>,
    weak: std::sync::Weak<Document>,
    flag: &std::sync::atomic::AtomicBool,
    worker: std::thread::JoinHandle<()>,
) {
}
pub fn generic<T: Into<String>>(value: T) -> T {
    value
}
pub fn opaque() -> impl Iterator<Item = u32> {
    std::iter::empty()
}
pub fn trait_object(renderer: &dyn Renderer) {}
pub(self) fn private_self(page: u32) {}
fn private(page: u32) -> String {
    String::new()
}
#[unsafe(no_mangle)]
extern "system" fn private_jni(handle: i64) {}

impl Renderer for Document {
    fn render(&self, scale: f32) {}
}

impl std::fmt::Display for Title {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

#[allow(raw_type_in_signature)]
pub fn allowed_explicitly(page: u32) {}

fn main() {}
