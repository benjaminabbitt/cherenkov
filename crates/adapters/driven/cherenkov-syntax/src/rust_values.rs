// Derived from cargo-mutants (https://github.com/sourcefrog/cargo-mutants),
// src/fnvalue.rs, 27.1.0. Copyright (c) 2021 Martin Pool, MIT licence; the
// notice is reproduced in THIRD-PARTY-NOTICES.md at the repository root and
// must travel with copies of this file.

//! Function-body replacement values for Rust result types, as cargo-mutants
//! 27.1.0 generates them (`src/fnvalue.rs`): a small parser for the type's
//! source text and the same recursive value table, branch for branch.

/// The replacement values for a function whose result type is
/// `return_type` (`None`: no `->`, the unit type). `error_exprs` are the
/// configured `Err` values for `Result`. An unparseable type yields
/// `Default::default()`, as cargo-mutants does for unrecognized types.
pub fn return_values(return_type: Option<&str>, error_exprs: &[&str]) -> Vec<String> {
    let Some(text) = return_type else {
        return vec!["()".into()];
    };
    match Parser::new(text).whole() {
        Some(t) => values(&t, error_exprs),
        None => vec!["Default::default()".into()],
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ty {
    Path(Vec<Segment>),
    Reference {
        mutable: bool,
        elem: Box<Ty>,
    },
    Slice(Box<Ty>),
    Array(Box<Ty>, String),
    Tuple(Vec<Ty>),
    /// `impl A + B`: the bounds' paths.
    ImplTrait(Vec<Vec<Segment>>),
    Never,
    /// Anything else (`dyn`, `fn`, raw pointers): `Default::default()`.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    ident: String,
    args: Vec<Arg>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Arg {
    Lifetime,
    Type(Ty),
    /// `Item = T` in a trait bound.
    Assoc(String, Ty),
    /// A const or anything else.
    Other,
}

fn values(t: &Ty, errs: &[&str]) -> Vec<String> {
    let each = |inner: &Ty, f: &dyn Fn(String) -> String| {
        values(inner, errs).into_iter().map(f).collect::<Vec<_>>()
    };
    match t {
        Ty::Path(path) => path_values(path, errs),
        Ty::Array(elem, len) => each(elem, &|r| format!("[{r}; {len}]")),
        Ty::Slice(elem) => leaked_vec(&values(elem, errs)),
        Ty::Reference { mutable, elem } => match &**elem {
            Ty::Path(p) if !mutable && is_ident(p, "str") => {
                vec![r#""""#.into(), r#""xyzzy""#.into()]
            }
            Ty::Slice(inner) => leaked_vec(&values(inner, errs)),
            other => each(other, &|r| format!("Box::leak(Box::new({r}))")),
        },
        Ty::Tuple(elems) => {
            let mut combos: Vec<Vec<String>> = vec![Vec::new()];
            for elem in elems {
                let reps = values(elem, errs);
                combos = combos
                    .iter()
                    .flat_map(|prefix| {
                        reps.iter()
                            .map(move |r| [prefix.clone(), vec![r.clone()]].concat())
                    })
                    .collect();
            }
            combos
                .into_iter()
                .map(|c| format!("({})", c.join(", ")))
                .collect()
        }
        Ty::ImplTrait(bounds) => match bounds.iter().find_map(|b| iterator_item(b)) {
            Some(item) => std::iter::once("::std::iter::empty()".to_string())
                .chain(
                    values(item, errs)
                        .into_iter()
                        .map(|r| format!("::std::iter::once({r})")),
                )
                .collect(),
            None => vec![],
        },
        Ty::Never => vec![],
        Ty::Other => vec!["Default::default()".into()],
    }
}

fn leaked_vec(reps: &[String]) -> Vec<String> {
    std::iter::once("Vec::leak(Vec::new())".to_string())
        .chain(reps.iter().map(|r| format!("Vec::leak(vec![{r}])")))
        .collect()
}

const UNSIGNED: &[&str] = &["u8", "u16", "u32", "u64", "u128", "usize"];
const SIGNED: &[&str] = &["i8", "i16", "i32", "i64", "i128", "isize"];
const NONZERO_SIGNED: &[&str] = &[
    "NonZeroIsize",
    "NonZeroI8",
    "NonZeroI16",
    "NonZeroI32",
    "NonZeroI64",
    "NonZeroI128",
];
const NONZERO_UNSIGNED: &[&str] = &[
    "NonZeroUsize",
    "NonZeroU8",
    "NonZeroU16",
    "NonZeroU32",
    "NonZeroU64",
    "NonZeroU128",
];

fn path_values(path: &[Segment], errs: &[&str]) -> Vec<String> {
    let last = path.last().expect("a path has at least one segment");
    scalar_values(path, last)
        .or_else(|| generic_values(last, errs))
        .unwrap_or_else(|| vec!["Default::default()".into()])
}

/// The values of a primitive, string or non-zero integer path; none for any
/// other path.
fn scalar_values(path: &[Segment], last: &Segment) -> Option<Vec<String>> {
    let one = |s: &[&str]| s.iter().any(|i| is_ident(path, i));
    let values: Vec<&str> = if is_ident(path, "bool") {
        vec!["true", "false"]
    } else if is_ident(path, "String") {
        vec!["String::new()", r#""xyzzy".into()"#]
    } else if is_ident(path, "str") {
        vec![r#""""#, r#""xyzzy""#]
    } else if one(UNSIGNED) {
        vec!["0", "1"]
    } else if one(SIGNED) {
        vec!["0", "1", "-1"]
    } else if NONZERO_SIGNED.contains(&last.ident.as_str()) {
        vec!["1.try_into().unwrap()", "(-1).try_into().unwrap()"]
    } else if NONZERO_UNSIGNED.contains(&last.ident.as_str()) {
        vec!["1.try_into().unwrap()"]
    } else if one(&["f32", "f64"]) {
        vec!["0.0", "1.0", "-1.0"]
    } else {
        return None;
    };
    Some(values.into_iter().map(String::from).collect())
}

/// The values of a generic path (`Result`, `Option`, a collection, a smart
/// pointer, `HttpResponse`) from those of its arguments; none for any other.
fn generic_values(last: &Segment, errs: &[&str]) -> Option<Vec<String>> {
    let map_each = |inner: &Ty, f: &dyn Fn(&str) -> String| -> Vec<String> {
        values(inner, errs).iter().map(|r| f(r)).collect()
    };
    let name = &last.ident;
    if let Some(t) = first_type_arg(last, "NonZero") {
        Some(generic_nonzero_values(t))
    } else if last.ident == "Result" {
        let ok = match first_type_arg(last, "Result") {
            Some(t) => map_each(t, &|r| format!("Ok({r})")),
            None => vec!["Ok(Default::default())".into()],
        };
        Some(
            ok.into_iter()
                .chain(errs.iter().map(|e| format!("Err({e})")))
                .collect(),
        )
    } else if last.ident == "HttpResponse" {
        Some(vec!["HttpResponse::Ok().finish()".into()])
    } else if let Some(t) = first_type_arg(last, "Option") {
        Some(
            std::iter::once("None".to_string())
                .chain(map_each(t, &|r| format!("Some({r})")))
                .collect(),
        )
    } else if let Some(t) = first_type_arg(last, "Vec") {
        Some(
            std::iter::once("vec![]".to_string())
                .chain(map_each(t, &|r| format!("vec![{r}]")))
                .collect(),
        )
    } else if let Some(t) = first_type_arg(last, "Cow") {
        Some(
            values(t, errs)
                .iter()
                .flat_map(|r| {
                    [
                        format!("Cow::Borrowed({r})"),
                        format!("Cow::Owned({r}.to_owned())"),
                    ]
                })
                .collect(),
        )
    } else if let Some(t) = only_type_arg(last, &["Box", "Cell", "RefCell", "Arc", "Rc", "Mutex"]) {
        Some(map_each(t, &|r| format!("{name}::new({r})")))
    } else if let Some(t) = only_type_arg(
        last,
        &[
            "BinaryHeap",
            "BTreeSet",
            "HashSet",
            "LinkedList",
            "VecDeque",
        ],
    ) {
        Some(
            std::iter::once(format!("{name}::new()"))
                .chain(map_each(t, &|r| format!("{name}::from_iter([{r}])")))
                .collect(),
        )
    } else if let Some((k, v)) = map_args(last) {
        let (ks, vs) = (values(k, errs), values(v, errs));
        Some(
            std::iter::once(format!("{name}::new()"))
                .chain(ks.iter().flat_map(|k| {
                    vs.iter()
                        .map(move |v| format!("{name}::from_iter([({k}, {v})])"))
                }))
                .collect(),
        )
    } else {
        single_type_arg(last).map(|t| {
            std::iter::once(format!("{name}::new()"))
                .chain(values(t, errs).iter().flat_map(|r| {
                    [
                        format!("{name}::from_iter([{r}])"),
                        format!("{name}::new({r})"),
                        format!("{name}::from({r})"),
                    ]
                }))
                .collect()
        })
    }
}

/// The values of `NonZero<T>` (cargo-mutants 27.1.0): only positive when `T`
/// is a bare unsigned integer, either sign for a signed or unknown `T`.
fn generic_nonzero_values(inner: &Ty) -> Vec<String> {
    let unsigned = matches!(inner, Ty::Path(p) if UNSIGNED.iter().any(|u| is_ident(p, u)));
    let mut values = vec!["1.try_into().unwrap()".to_string()];
    if !unsigned {
        values.push("(-1).try_into().unwrap()".into());
    }
    values
}

/// `path.is_ident(name)`: exactly one segment, with no arguments.
fn is_ident(path: &[Segment], name: &str) -> bool {
    matches!(path, [s] if s.ident == name && s.args.is_empty())
}

/// The first type argument of `seg` when it is `name`; lifetimes are
/// skipped, and any other kind of argument first means none.
fn first_type_arg<'a>(seg: &'a Segment, name: &str) -> Option<&'a Ty> {
    if seg.ident != name {
        return None;
    }
    for arg in &seg.args {
        match arg {
            Arg::Type(t) => return Some(t),
            Arg::Lifetime => {}
            _ => return None,
        }
    }
    None
}

/// `seg` is one of `names` with exactly one argument, a type.
fn only_type_arg<'a>(seg: &'a Segment, names: &[&str]) -> Option<&'a Ty> {
    match (names.contains(&seg.ident.as_str()), seg.args.as_slice()) {
        (true, [Arg::Type(t)]) => Some(t),
        _ => None,
    }
}

/// `BTreeMap<K, V>` or `HashMap<K, V>`: exactly two type arguments.
fn map_args(seg: &Segment) -> Option<(&Ty, &Ty)> {
    match (
        ["BTreeMap", "HashMap"].contains(&seg.ident.as_str()),
        seg.args.as_slice(),
    ) {
        (true, [Arg::Type(k), Arg::Type(v)]) => Some((k, v)),
        _ => None,
    }
}

/// Exactly one type argument, ignoring other kinds of argument.
fn single_type_arg(seg: &Segment) -> Option<&Ty> {
    let types: Vec<&Ty> = seg
        .args
        .iter()
        .filter_map(|a| match a {
            Arg::Type(t) => Some(t),
            _ => None,
        })
        .collect();
    match types.as_slice() {
        [t] => Some(t),
        _ => None,
    }
}

/// `Iterator<Item = T>` as the last segment of a bound: `T`.
fn iterator_item(bound: &[Segment]) -> Option<&Ty> {
    let last = bound.last()?;
    if last.ident != "Iterator" {
        return None;
    }
    match last.args.first() {
        Some(Arg::Assoc(name, ty)) if name == "Item" => Some(ty),
        _ => None,
    }
}

/// A recursive-descent parser over the type's source text.
struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Parser {
            s: text.as_bytes(),
            i: 0,
        }
    }

    /// The whole text as one type, or `None`.
    fn whole(&mut self) -> Option<Ty> {
        let t = self.ty()?;
        self.ws();
        (self.i == self.s.len()).then_some(t)
    }

    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(u8::is_ascii_whitespace) {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, token: &str) -> bool {
        self.ws();
        if self.s[self.i..].starts_with(token.as_bytes()) {
            self.i += token.len();
            true
        } else {
            false
        }
    }

    /// A keyword followed by a non-identifier character.
    fn eat_word(&mut self, word: &str) -> bool {
        self.ws();
        let end = self.i + word.len();
        if self.s[self.i..].starts_with(word.as_bytes())
            && !self
                .s
                .get(end)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
        {
            self.i = end;
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> Option<String> {
        self.ws();
        let start = self.i;
        while self
            .s
            .get(self.i)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
        {
            self.i += 1;
        }
        (self.i > start).then(|| String::from_utf8_lossy(&self.s[start..self.i]).into_owned())
    }

    fn lifetime(&mut self) -> bool {
        if self.peek() == Some(b'\'') {
            self.i += 1;
            self.ident();
            true
        } else {
            false
        }
    }

    fn ty(&mut self) -> Option<Ty> {
        match self.peek()? {
            b'!' => {
                self.i += 1;
                Some(Ty::Never)
            }
            b'&' => {
                self.i += 1;
                self.lifetime();
                let mutable = self.eat_word("mut");
                Some(Ty::Reference {
                    mutable,
                    elem: Box::new(self.ty()?),
                })
            }
            b'[' => {
                self.i += 1;
                let elem = self.ty()?;
                if self.eat(";") {
                    let start = self.i;
                    while self.s.get(self.i).is_some_and(|c| *c != b']') {
                        self.i += 1;
                    }
                    let len = String::from_utf8_lossy(&self.s[start..self.i])
                        .trim()
                        .to_string();
                    self.eat("]").then(|| Ty::Array(Box::new(elem), len))
                } else {
                    self.eat("]").then(|| Ty::Slice(Box::new(elem)))
                }
            }
            b'(' => {
                self.i += 1;
                let mut elems = Vec::new();
                let mut comma = false;
                while !self.eat(")") {
                    elems.push(self.ty()?);
                    comma = self.eat(",");
                    if !comma && self.peek() != Some(b')') {
                        return None;
                    }
                }
                // `(T)` is T in parentheses; `(T,)` is a tuple.
                match (elems.len(), comma) {
                    (1, false) => elems.pop(),
                    _ => Some(Ty::Tuple(elems)),
                }
            }
            b'*' => {
                // Raw pointers: not in cargo-mutants' table.
                self.i += 1;
                if !(self.eat_word("const") || self.eat_word("mut")) {
                    return None;
                }
                self.ty().map(|_| Ty::Other)
            }
            _ if self.eat_word("impl") => {
                let mut bounds = vec![self.path()?];
                while self.eat("+") {
                    if !self.lifetime() {
                        bounds.push(self.path()?);
                    }
                }
                Some(Ty::ImplTrait(bounds))
            }
            _ if self.eat_word("dyn") => {
                self.path()?;
                while self.eat("+") {
                    if !self.lifetime() {
                        self.path()?;
                    }
                }
                Some(Ty::Other)
            }
            _ if self.eat_word("fn") => {
                // Function pointers: skip the rest; not in the table.
                self.i = self.s.len();
                Some(Ty::Other)
            }
            _ => self.path().map(Ty::Path),
        }
    }

    fn path(&mut self) -> Option<Vec<Segment>> {
        self.eat("::");
        let mut segments = vec![self.segment()?];
        while self.eat("::") {
            segments.push(self.segment()?);
        }
        Some(segments)
    }

    fn segment(&mut self) -> Option<Segment> {
        let ident = self.ident()?;
        let mut args = Vec::new();
        if self.eat("(") {
            // Fn-trait sugar, `Fn(A, B) -> R`: a path segment like any other,
            // whose arguments no replacement depends on.
            while !self.eat(")") {
                self.ty()?;
                if !self.eat(",") && self.peek() != Some(b')') {
                    return None;
                }
            }
            if self.eat("->") {
                self.ty()?;
            }
        } else if self.eat("<") {
            while !self.eat(">") {
                args.push(self.arg()?);
                if !self.eat(",") && self.peek() != Some(b'>') {
                    return None;
                }
            }
        }
        Some(Segment { ident, args })
    }

    fn arg(&mut self) -> Option<Arg> {
        if self.lifetime() {
            return Some(Arg::Lifetime);
        }
        if self
            .peek()
            .is_some_and(|c| c.is_ascii_digit() || c == b'{' || c == b'-')
        {
            // A const argument: skip to the next `,` or `>` at this depth.
            let mut depth = 0i32;
            while let Some(&c) = self.s.get(self.i) {
                match c {
                    b'{' | b'(' | b'[' => depth += 1,
                    b'}' | b')' | b']' => depth -= 1,
                    b',' | b'>' if depth == 0 => break,
                    _ => {}
                }
                self.i += 1;
            }
            return Some(Arg::Other);
        }
        let save = self.i;
        if let Some(name) = self.ident()
            && self.peek() == Some(b'=')
            && !self.s[self.i..].starts_with(b"==")
        {
            self.i += 1;
            return Some(Arg::Assoc(name, self.ty()?));
        }
        self.i = save;
        self.ty().map(Arg::Type)
    }
}
