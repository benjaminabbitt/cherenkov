//! One function per cargo-mutants genre, for the fidelity tests: every
//! run-time mutant here must behave like the same mutant compiled from source.

pub fn compare(a: i64, b: i64) -> i64 {
    let mut n = 0;
    if a < b {
        n += 1;
    }
    if a <= b {
        n += 2;
    }
    if a > b {
        n += 4;
    }
    if a >= b {
        n += 8;
    }
    if a == b {
        n += 16;
    }
    if a != b {
        n += 32;
    }
    n
}

pub fn logic(p: bool, q: bool) -> i64 {
    if p && q {
        return 1;
    }
    if p || q {
        return 2;
    }
    3
}

pub fn arith(a: i64, b: i64) -> i64 {
    let s = a + b;
    let d = a - b;
    let m = a * b;
    let q = a / b;
    let r = a % b;
    s * 10000 + d * 1000 + m * 100 + q * 10 + r
}

pub fn bits(a: u32, b: u32) -> u32 {
    let x = a & b;
    let y = a | b;
    let z = a ^ b;
    let l = a << 2;
    let h = a >> 1;
    x + y * 16 + z * 256 + l * 4096 + h * 65536
}

pub fn compound(mut t: i64, k: i64) -> i64 {
    t += k;
    t -= 1;
    t *= k;
    t /= 2;
    t %= 1000;
    t
}

pub fn shifts(mut v: u32) -> u32 {
    v <<= 3;
    v >>= 1;
    v &= 0xff;
    v |= 0x100;
    v ^= 0x3;
    v
}

pub fn unary(n: i64, flag: bool) -> i64 {
    let m = -n;
    if !flag {
        return m;
    }
    m * 2
}

pub fn inline(a: i64, b: i64) -> i64 {
    sum3(a + b, a * b, a - b)
}

fn sum3(x: i64, y: i64, z: i64) -> i64 {
    x * 100 + y * 10 + z
}

pub fn classify(x: Option<i64>) -> i64 {
    match x {
        Some(n) if n > 10 => 1,
        Some(_) => 2,
        None => 3,
    }
}

pub fn digit(k: u8) -> u8 {
    match k {
        0 => 10,
        1 => 11,
        2 => 12,
        _ => 0,
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct Pair {
    pub a: i64,
    pub b: i64,
}

pub fn pair(a: i64) -> Pair {
    Pair {
        a,
        ..Default::default()
    }
}

pub fn name() -> String {
    String::from("ops")
}

pub fn maybe(n: i64) -> Option<i64> {
    if n > 0 { Some(n) } else { None }
}

pub fn list(n: usize) -> Vec<u8> {
    vec![1; n]
}

pub fn parse(s: &str) -> Result<i64, String> {
    s.parse::<i64>().map_err(|e| e.to_string())
}

pub fn flag(n: i64) -> bool {
    n % 2 == 0
}

impl Pair {
    /// The names of the fields that are set: a method, whose result type
    /// is on its declaration in the `impl`, not on its code.
    pub fn set(&self) -> Vec<String> {
        let mut set = Vec::new();
        if self.a != 0 {
            set.push("a".to_string());
        }
        if self.b != 0 {
            set.push("b".to_string());
        }
        set
    }
}

/// Chains whose swapped operator regroups the source: `a || b && c` is
/// `a || (b && c)`, as a value and as a condition.
pub fn all3(a: bool, b: bool, c: bool) -> bool {
    a && b && c
}

pub fn any3(a: bool, b: bool, c: bool) -> i64 {
    if a || b || c { 1 } else { 0 }
}

/// Arithmetic whose swapped operator regroups the source: `a - b * c`
/// with `-` as `/` is `a / b * c`, that is `(a / b) * c`.
pub fn mix(a: i64, b: i64, c: i64) -> i64 {
    a - b * c
}

pub fn check(n: i64) -> Result<(), String> {
    if n < 0 { Err(format!("{n} is negative")) } else { Ok(()) }
}

/// A mutated comparison inside a closure: the line has code in both the
/// function and the closure.
pub fn evens(v: &[i64]) -> usize {
    v.iter().filter(|x| *x % 2 == 0).count()
}

/// A struct with no `Default`: cargo-mutants' `Default::default()` for it
/// does not compile.
#[derive(Debug, PartialEq)]
pub struct Plain {
    pub n: i64,
}

pub fn plain(n: i64) -> Plain {
    Plain { n }
}

/// A field-less enum without `Default` (DWARF's `DW_TAG_enumeration_type`).
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Shade {
    Light,
    Dark,
}

pub fn shade(n: i64) -> Shade {
    if n > 0 { Shade::Dark } else { Shade::Light }
}

/// `!` folded into the branch it decides: no `xor`, an inverted jump.
pub fn absent(x: Option<i64>) -> i64 {
    if !matches!(x, Some(_)) { 1 } else { 0 }
}

/// Comparisons whose result is a value (`setcc`), unsigned and signed.
pub fn more(a: u64, b: u64) -> bool {
    a > b
}

pub fn less(a: i64, b: i64) -> bool {
    a < b
}

/// A derived Default nothing calls: inline, so never compiled here.
#[derive(Default, Debug, PartialEq)]
pub struct Unused {
    pub n: i64,
    pub s: String,
}

pub fn unused(n: i64) -> Unused {
    Unused {
        n,
        s: "set".to_string(),
    }
}

// --- Shapes from the adversarial review: each a mutant the debugger once
// got wrong. Their tests pin the inputs that tell the right mutant apart.

/// A generic used at two types: only the second's test detects a mutant.
pub fn pick<T: Copy>(a: T, b: T, first: bool) -> T {
    if first { a } else { b }
}

fn parse_note(n: u32) -> u32 {
    n
}

/// A statement with `?`: deleting it must resume after the success path.
pub fn noted(s: &str) -> Result<u32, std::num::ParseIntError> {
    parse_note(s.parse::<u32>()?);
    Ok(1)
}

/// `!` on an integer is a bitwise not, not a bool's.
pub fn invert(b: u8) -> u8 {
    !b
}

/// A generic struct with a hand-written Default: not an empty vector.
pub struct Pool<T> {
    pub items: Vec<T>,
    pub limit: usize,
}

impl<T> Default for Pool<T> {
    fn default() -> Self {
        Pool {
            items: Vec::new(),
            limit: 10,
        }
    }
}

pub fn pool() -> Pool<u8> {
    Pool {
        items: vec![1],
        limit: 3,
    }
}

/// A 128-bit result: zeroing it zeroes both halves.
pub fn wide(a: u128, b: u128) -> u128 {
    a + b
}

/// One-line guarded assignment: the mutant applies only when guarded.
pub fn guarded(f: bool) -> i64 {
    let mut s = 1;
    if f { s = s + 2; }
    s
}

pub struct Counter {
    pub n: i64,
}

impl Counter {
    pub fn bump(&mut self) -> i64 {
        self.n += 1;
        self.n
    }
}

/// A returned value that calls a method with a side effect.
pub fn bumped(c: &mut Counter) -> i64 {
    c.bump() + 10
}

/// Unsigned arithmetic: rustc checks it for overflow, and a swapped
/// operator must be checked as the source's would be.
pub fn inc(x: u32) -> u32 {
    let y = x + 1;
    y
}

pub fn total(a: u32, b: u32) -> u32 {
    a + b
}

/// Through an alias the generic is hidden: cargo-mutants returns
/// `Default::default()`, which is Pool's own (limit 10).
pub type Cache = Pool<u8>;

pub fn cache() -> Cache {
    Pool {
        items: vec![1],
        limit: 3,
    }
}

// --- Async fns: rustc compiles each into a function that only makes its
// future and the future's poll, which runs the body.

pub async fn twice(n: i64) -> i64 {
    n * 2
}

pub async fn validate(n: i64) -> Result<(), String> {
    if n < 0 {
        return Err(format!("{n} is negative"));
    }
    Ok(())
}

pub async fn fetch(n: usize) -> Result<Vec<u8>, String> {
    if n > 100 {
        return Err("too many".to_string());
    }
    Ok(vec![7; n])
}

/// An async fn that awaits another.
pub async fn twice_plus(n: i64) -> i64 {
    let t = twice(n).await;
    t + 1
}

pub async fn paired(n: i64) -> Result<Pair, String> {
    Ok(Pair { a: n, b: 1 })
}

// --- Std values the program never builds itself: cargo-mutants' values
// for them need constructors the test binary does not contain.

pub fn tags(n: u32) -> std::collections::HashSet<u32> {
    (n..n + 3).collect()
}

pub fn shared(n: i64) -> std::sync::Arc<i64> {
    std::sync::Arc::new(n * 2)
}

pub fn home(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from("/home").join(name)
}

pub fn limit() -> &'static u32 {
    static LIMIT: u32 = 7;
    &LIMIT
}

pub fn boxed(n: i64) -> Box<i64> {
    Box::new(n + 1)
}

pub async fn shared_later(n: i64) -> std::sync::Arc<i64> {
    std::sync::Arc::new(n + 5)
}

// --- Async fns whose body a macro compiles into an async block: boxed, as
// `#[async_trait]` does, or awaited inside the fn, as
// `#[tracing::instrument]` does. The block's poll runs the body.

#[ops_futures::boxed]
pub async fn checked_later(n: i64) -> Result<u32, String> {
    if n < 0 {
        return Err(format!("{n} is negative"));
    }
    Ok(n as u32 + 1)
}

#[ops_futures::boxed]
pub async fn named_later(n: i64) -> Option<String> {
    (n > 0).then(|| format!("n{n}"))
}

#[ops_futures::wrapped]
pub async fn listed_later(n: usize) -> Result<Vec<u8>, String> {
    if n > 100 {
        return Err("too many".to_string());
    }
    Ok(vec![3; n])
}

// --- Private project types: the support library cannot name them, so it
// mirrors them from the program's DWARF.

#[derive(Default, Debug, PartialEq, Clone)]
pub struct Label {
    pub text: String,
    pub weight: u8,
}

#[derive(Default, Debug, PartialEq)]
pub struct Inventory {
    pub labels: Vec<Label>,
    pub counts: std::collections::HashMap<String, u32>,
    pub owner: Label,
    pub id: u16,
}

pub fn inventory(n: u16) -> Inventory {
    let labels: Vec<Label> = (0..n).map(|i| Label { text: format!("l{i}"), weight: 1 }).collect();
    let counts = labels.iter().map(|l| (l.text.clone(), 1)).collect();
    Inventory { labels, counts, owner: Label::default(), id: n }
}

pub fn labels(n: u8) -> Vec<Label> {
    (0..n).map(|i| Label { text: format!("l{i}"), weight: i }).collect()
}

pub fn stock(n: u16) -> Result<Inventory, String> {
    if n > 100 {
        return Err("too many".into());
    }
    Ok(inventory(n))
}

pub fn shared_label(n: u8) -> std::sync::Arc<Label> {
    std::sync::Arc::new(Label { text: "x".repeat(n.into()), weight: n })
}

pub async fn stock_later(n: u16) -> Result<Inventory, String> {
    stock(n)
}

/// A hand-written Default: only the program's own code makes it.
#[derive(Debug, PartialEq)]
pub struct Tuned {
    pub level: u32,
    pub name: String,
}

impl Default for Tuned {
    fn default() -> Self {
        Tuned { level: 7, name: "tuned".into() }
    }
}

pub fn tuned_all(n: usize) -> Vec<Tuned> {
    (0..n).map(|_| Tuned::default()).collect()
}

/// An enum with a plain tag: no field has a niche.
#[derive(Debug, PartialEq)]
pub enum Event {
    Started(u32),
    Moved { x: i64, y: i64 },
    Stopped,
}

pub fn event_index(n: u32) -> std::collections::HashMap<u32, Event> {
    (0..n).map(|i| (i, if i % 2 == 0 { Event::Started(i) } else { Event::Moved { x: 1, y: 2 } })).collect()
}

/// An enum whose tag lives in its String's niche.
#[derive(Debug, PartialEq)]
pub enum Slot {
    Empty,
    Full(String),
}

pub fn slot_index(n: u32) -> std::collections::HashMap<u32, Slot> {
    (0..n).map(|i| (i, if i == 0 { Slot::Empty } else { Slot::Full(i.to_string()) })).collect()
}

/// Two types of one name in one crate: only the generated one (as prost's
/// messages, outside the sources) has a Default.
pub mod generated;

pub fn generated_letter(id: u64) -> generated::Letter {
    generated::Letter { body: vec![1], id }
}

/// Not mirrored (a `dyn` field), so only its layout could build it.
#[derive(Debug)]
pub struct Letter {
    pub to: String,
    pub urgent: bool,
    pub note: Option<Box<dyn std::fmt::Debug>>,
}

pub fn letter(to: &str) -> Letter {
    Letter { to: to.to_string(), urgent: true, note: None }
}

/// A function pointer and a trait object: mirrored as any of their kind.
#[derive(Default)]
pub struct Hooks {
    pub on: Option<fn(u32) -> u32>,
    pub name: String,
    pub extra: Option<Box<dyn Fn() + Send>>,
}

pub fn hooks(n: u32) -> Vec<Hooks> {
    (0..n)
        .map(|i| Hooks { on: [None, Some(double as fn(u32) -> u32)][i.min(1) as usize], name: String::new(), extra: None })
        .collect()
}

fn double(x: u32) -> u32 {
    x * 2
}

/// Array fields: plain, nested, empty, and of a mirrored element.
#[derive(Debug, Default, PartialEq)]
pub struct Gauge {
    pub bins: [u64; 2],
    pub grid: [[u16; 3]; 2],
    pub none: [u8; 0],
    pub marks: [Mark; 2],
}

#[derive(Debug, Default, PartialEq)]
pub struct Mark {
    pub at: u32,
}

pub fn gauges(n: u32) -> Vec<Gauge> {
    (0..n)
        .map(|i| Gauge { bins: [1, 2], grid: [[3; 3]; 2], none: [], marks: [Mark { at: i }, Mark { at: 7 }] })
        .collect()
}

/// Borrowing fields, which a struct can hold only for `'static`.
#[derive(Debug, Default, PartialEq)]
pub struct Banner {
    pub text: std::borrow::Cow<'static, str>,
    pub by: &'static str,
}

pub fn banners(n: u32) -> Vec<Banner> {
    (0..n).map(|_| Banner { text: "hi".into(), by: "ops" }).collect()
}

// --- Arithmetic inside something: a call argument, a field, a guard, an
// entry. The instruction is told from the line's others by its column.

fn times(x: i64, k: i64) -> i64 {
    x * k
}

pub fn lifted(a: i64) -> i64 {
    times(a + 1, 2)
}

fn same(x: u64) -> u64 {
    x
}

pub fn lifted_u(s: u64) -> u64 {
    same(s + 1)
}

fn positive(x: i64) -> bool {
    x > 0
}

/// A test that does not tell `a + 1` from `a - 1` for the large `a`.
pub fn lifted_loose(a: i64) -> bool {
    positive(a + 1)
}

pub struct Tally {
    pub x: i64,
    pub u: u64,
    pub w: f64,
    pub bits: u32,
    pub seen: std::collections::BTreeMap<u32, i32>,
}

impl Tally {
    pub fn new() -> Tally {
        Tally {
            x: 0,
            u: 0,
            w: 0.0,
            bits: 0,
            seen: std::collections::BTreeMap::new(),
        }
    }
}

pub fn tally_x(s: &mut Tally) {
    s.x += 1;
}

pub fn tally_u(s: &mut Tally) {
    s.u += 1;
}

pub fn tally_down(s: &mut Tally) {
    s.u -= 1;
    s.x -= 2;
}

pub fn tally_guarded(m: &std::sync::Mutex<Tally>) {
    let mut g = m.lock().unwrap();
    g.x += 1;
}

pub fn tally_seen(s: &mut Tally, k: u32) {
    *s.seen.entry(k).or_default() += 1;
}

pub fn tally_w(s: &mut Tally) {
    s.w += 0.5;
    s.w *= 3.0;
}

pub fn tally_bits(s: &mut Tally) {
    s.bits |= 4;
    s.bits &= 0xfe;
}

fn keep_f(x: f64) -> f64 {
    x
}

pub fn blend(a: f64, b: f64) -> f64 {
    keep_f(a - b)
}

pub fn area(w: f64, h: f64) -> f64 {
    keep_f(w * h)
}

/// Functions whose result is a trivial value's type: the stubs of tier P
/// return `1`, `-1`, `None` and `Ok(())` of each of these exactly.
pub fn sign(n: i64) -> i64 {
    n.signum()
}

pub fn narrow(n: i64) -> i32 {
    n.clamp(0, 5) as i32
}

pub fn small(n: i64) -> u16 {
    n.clamp(0, 9) as u16
}

pub fn verdict(n: i64) -> Option<bool> {
    if n < 0 { None } else { Some(n.is_positive()) }
}

pub fn index_of(n: i64) -> Option<u32> {
    u32::try_from(n).ok()
}

pub fn status(n: i64) -> Result<(), u8> {
    if n < 0 { Err(7) } else { Ok(()) }
}

pub fn fits(n: i64) -> Result<(), std::num::TryFromIntError> {
    u8::try_from(n).map(|_| ())
}

// --- Conditions that contain calls, and comparisons that are values. Each
// comparison's flags are read by the instruction positioned at its own
// operands, whatever else its line holds.

pub struct Feed {
    pub cursor: usize,
    pub out: Option<u32>,
    pub handled: u64,
}

pub fn drained(v: &Vec<u32>, f: bool) -> i64 {
    if v.is_empty() && !f { 1 } else { 2 }
}

pub fn pending(s: &Feed) -> i64 {
    if s.cursor > 0 || s.out.is_some() {
        10
    } else {
        20
    }
}

pub fn busy(feeds: &[Feed]) -> usize {
    feeds.iter().filter(|t| t.handled >= 12).count()
}

pub fn enough(v: &Vec<u32>) -> bool {
    v.len() >= 3
}

pub fn fewer(v: &Vec<u32>, n: usize) -> i64 {
    if v.len() < n { 1 } else { 2 }
}

pub fn keyed(m: &std::collections::BTreeMap<u32, u32>, k: u32) -> i64 {
    if m.contains_key(&k) && m.len() > 1 { 1 } else { 2 }
}

pub fn large(o: Option<u64>) -> bool {
    o.unwrap_or(0) >= 100
}

fn count_of(v: &[u32]) -> usize {
    v.len()
}

pub fn both(v: &[u32], limit: usize) -> bool {
    count_of(v) > 0 && count_of(v) <= limit
}

pub fn either(v: &[u32], w: &[u32]) -> bool {
    count_of(v) > 2 || count_of(w) > 2
}

pub fn above(a: f64, b: f64) -> i64 {
    if a > b { 1 } else { 2 }
}

pub fn same_len(v: &Vec<u32>, w: &Vec<u32>) -> i64 {
    if v.len() == w.len() { 1 } else { 2 }
}

// Deleted match arms, as deps' `RemoteRef::parse`, `sorted` and
// `remotes_mut` have them: a catch-all whose body's first character has no
// code (`return None` makes its code at `None`), a block body whose first
// code is on a later line, a body that is only a name the pattern bound.
pub fn urlish(class: u8, host: &str, path: &str) -> Option<String> {
    let url = match class {
        0 => format!("https://{host}{path}"),
        1 => format!("file://{path}"),
        _ => return None,
    };
    Some(url)
}

pub fn ordered(key: &str, v: &[u32]) -> Vec<u32> {
    match (key, v) {
        ("up" | "sorted", items) => {
            let mut out = items.to_vec();
            out.sort();
            out
        }
        _ => v.to_vec(),
    }
}

pub fn bound(v: &mut Option<Vec<u32>>) -> &mut Vec<u32> {
    match v {
        Some(entries) => entries,
        _ => unreachable!("a list is there"),
    }
}

// A closure inside a closure on one line, the `!` deleted from the outer
// one (deps' `Registry::forges`): the inner closure starts on the same line
// and is no copy of the outer.
pub fn unlisted(configured: &[(String, u8)], built_in: &[&str]) -> Vec<String> {
    built_in
        .iter()
        .filter(|label| !configured.iter().any(|(key, _)| key == *label))
        .map(|label| label.to_string())
        .collect()
}

// A guarded arm with an empty body (deps' `check`): the guard decides
// between going on past the match and the next arm, with no code in the arm
// to land on.
pub fn kinds_ok(kinds: &[Option<&str>], built_in: &[&str]) -> Result<(), String> {
    for kind in kinds {
        match kind {
            None => {}
            Some(k) if k.is_empty() || built_in.contains(k) => {}
            Some(k) => return Err(k.to_string()),
        }
    }
    Ok(())
}

// A `!` deleted in a closure that is on later lines of a multi-line
// expression statement (deps' `constraint_changes`): the closure has no code
// on the statement's first line, and rustc normalises the negated bool after
// copying it to the register the next call takes.
#[derive(Debug, PartialEq)]
pub struct Change {
    pub id: String,
    pub note: String,
}

pub fn dropped(entries: &[(String, bool)], limit: usize) -> Vec<Change> {
    entries
        .iter()
        .filter_map(|(id, held)| {
            let kept = *held || id.len() > limit || id.is_empty();
            (!kept).then(|| Change {
                id: id.clone(),
                note: id.to_uppercase(),
            })
        })
        .collect()
}
