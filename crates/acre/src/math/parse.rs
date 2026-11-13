//! The subset of LaTeX math that acre reads, parsed into a list of
//! atoms in the sense of *The TeXbook*, chapter 17.

use crate::scene::Font;

/// What an atom is to the spacing around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Ord,
    /// A large operator or an operator name, such as `\sum` or `\log`.
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
    /// A `\left ... \right` group.
    Inner,
}

/// The size a list is set at: display, text, script or scriptscript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Display,
    Text,
    Script,
    ScriptScript,
}

/// One item of a math list.
#[derive(Debug, Clone, PartialEq)]
pub enum Noad {
    Atom(Atom),
    /// A change of style for the rest of the list.
    Style(Style),
    /// A space in mu, eighteen to the em.
    Space(f64),
}

/// A nucleus with optional scripts.
#[derive(Debug, Clone, PartialEq)]
pub struct Atom {
    pub class: Class,
    pub nucleus: Nucleus,
    pub sup: Option<Vec<Noad>>,
    pub sub: Option<Vec<Noad>>,
    /// Whether, in display style, the scripts go above and below.
    pub limits: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Nucleus {
    /// Nothing, as in the base of `{}^2`.
    Empty,
    /// One character of the math font.
    Char(char),
    /// A large operator, set larger in display style.
    LargeOp(char),
    /// Upright text in `font`, from `\text`, `\mathrm` or an operator
    /// name.
    Text(String, Font),
    Group(Vec<Noad>),
    Frac(Vec<Noad>, Vec<Noad>),
    Sqrt {
        index: Option<Vec<Noad>>,
        radicand: Vec<Noad>,
    },
    /// A list between delimiters that grow to its height, `None` for
    /// `.`.
    Fenced {
        left: Option<char>,
        body: Vec<Noad>,
        right: Option<char>,
    },
}

impl Atom {
    fn new(class: Class, nucleus: Nucleus) -> Self {
        Atom {
            class,
            nucleus,
            sup: None,
            sub: None,
            limits: false,
        }
    }
}

/// `src`, LaTeX math, as a list of noads.
pub fn parse(src: &str) -> Result<Vec<Noad>, String> {
    let mut parser = Parser {
        chars: src.chars().collect(),
        at: 0,
    };
    parser.list(End::Eof)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    Eof,
    Brace,
    /// The `]` closing the index of `\sqrt[n]`.
    Bracket,
    Right,
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek();
        self.at += 1;
        c
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    /// The name after a `\` just read: a run of letters, or one other
    /// character.
    fn command_name(&mut self) -> Result<String, String> {
        let Some(c) = self.next() else {
            return Err("a `\\` at the end of the formula names nothing".into());
        };
        let mut name = String::from(c);
        if c.is_ascii_alphabetic() {
            while let Some(c) = self.peek().filter(char::is_ascii_alphabetic) {
                name.push(c);
                self.at += 1;
            }
        }
        Ok(name)
    }

    fn list(&mut self, end: End) -> Result<Vec<Noad>, String> {
        let mut out = Vec::new();
        loop {
            self.skip_spaces();
            let Some(c) = self.next() else {
                return match end {
                    End::Eof => Ok(out),
                    End::Brace => Err("a `{` is never closed".into()),
                    End::Bracket => Err("the `[` of a `\\sqrt` is never closed".into()),
                    End::Right => Err("a `\\left` has no `\\right`".into()),
                };
            };
            match c {
                '{' => {
                    let group = self.list(End::Brace)?;
                    out.push(Noad::Atom(Atom::new(Class::Ord, Nucleus::Group(group))));
                }
                '}' if end == End::Brace => return Ok(out),
                '}' => return Err("a `}` closes no `{`".into()),
                ']' if end == End::Bracket => return Ok(out),
                '^' | '_' => {
                    let script = self.argument()?;
                    attach(&mut out, c == '^', script)?;
                }
                '\\' => {
                    let name = self.command_name()?;
                    match name.as_str() {
                        "right" if end == End::Right => return Ok(out),
                        "right" => return Err("a `\\right` has no `\\left`".into()),
                        "limits" | "nolimits" => match out.last_mut() {
                            Some(Noad::Atom(a)) if a.class == Class::Op => {
                                a.limits = name == "limits";
                            }
                            _ => return Err(format!("`\\{name}` follows no operator")),
                        },
                        _ => out.push(self.command(&name)?),
                    }
                }
                c => out.push(char_noad(c)),
            }
        }
    }

    /// The argument of a command or a script: a braced list, or the one
    /// character or command that comes next.
    fn argument(&mut self) -> Result<Vec<Noad>, String> {
        self.skip_spaces();
        match self.next() {
            Some('{') => self.list(End::Brace),
            Some('\\') => {
                let name = self.command_name()?;
                Ok(vec![self.command(&name)?])
            }
            Some(c @ ('}' | '^' | '_' | ']')) => {
                Err(format!("an argument is missing before `{c}`"))
            }
            Some(c) => Ok(vec![char_noad(c)]),
            None => Err("an argument is missing at the end of the formula".into()),
        }
    }

    /// The text of a braced argument, as typed, with `\` taking the
    /// character after it.
    fn raw_argument(&mut self, command: &str) -> Result<String, String> {
        self.skip_spaces();
        if self.next() != Some('{') {
            return Err(format!("`\\{command}` takes its text in braces"));
        }
        let mut text = String::new();
        let mut depth = 0;
        loop {
            match self.next() {
                None => return Err("a `{` is never closed".into()),
                Some('}') if depth == 0 => return Ok(text),
                Some('}') => depth -= 1,
                Some('{') => depth += 1,
                Some('\\') => {
                    if let Some(c) = self.next() {
                        text.push(c);
                    }
                }
                Some(c) => text.push(c),
            }
        }
    }

    /// The delimiter after `\left` or `\right`.
    fn delimiter(&mut self, command: &str) -> Result<Option<char>, String> {
        self.skip_spaces();
        let missing = || format!("`\\{command}` needs a delimiter, such as `(` or `.`");
        let c = match self.next().ok_or_else(missing)? {
            '.' => return Ok(None),
            '\\' => {
                let name = self.command_name()?;
                match symbol(&name) {
                    Some((c, _)) if is_delimiter(c) => c,
                    _ => return Err(format!("`\\{name}` is not a delimiter")),
                }
            }
            c if is_delimiter(c) => c,
            c => return Err(format!("`{c}` is not a delimiter")),
        };
        Ok(Some(c))
    }

    fn command(&mut self, name: &str) -> Result<Noad, String> {
        let atom = |class, nucleus| Ok(Noad::Atom(Atom::new(class, nucleus)));
        match name {
            "frac" => {
                let num = self.argument()?;
                let den = self.argument()?;
                atom(Class::Ord, Nucleus::Frac(num, den))
            }
            "sqrt" => {
                self.skip_spaces();
                let index = if self.peek() == Some('[') {
                    self.at += 1;
                    Some(self.list(End::Bracket)?)
                } else {
                    None
                };
                let radicand = self.argument()?;
                atom(Class::Ord, Nucleus::Sqrt { index, radicand })
            }
            "left" => {
                let left = self.delimiter("left")?;
                let body = self.list(End::Right)?;
                let right = self.delimiter("right")?;
                atom(Class::Inner, Nucleus::Fenced { left, body, right })
            }
            "text" => {
                let text = self.raw_argument(name)?;
                atom(Class::Ord, Nucleus::Text(text, Font::Sans))
            }
            "mathrm" => {
                let text = self.raw_argument(name)?.replace(' ', "");
                atom(Class::Ord, Nucleus::Text(text, Font::Math))
            }
            "operatorname" => {
                let text = self.raw_argument(name)?.replace(' ', "");
                atom(Class::Op, Nucleus::Text(text, Font::Math))
            }
            "displaystyle" => Ok(Noad::Style(Style::Display)),
            "textstyle" => Ok(Noad::Style(Style::Text)),
            "scriptstyle" => Ok(Noad::Style(Style::Script)),
            "scriptscriptstyle" => Ok(Noad::Style(Style::ScriptScript)),
            "," => Ok(Noad::Space(3.0)),
            ":" | ">" => Ok(Noad::Space(4.0)),
            ";" => Ok(Noad::Space(5.0)),
            "!" => Ok(Noad::Space(-3.0)),
            " " => Ok(Noad::Space(6.0)),
            "quad" => Ok(Noad::Space(18.0)),
            "qquad" => Ok(Noad::Space(36.0)),
            _ => {
                if let Some(&(_, c, limits)) = LARGE_OPS.iter().find(|op| op.0 == name) {
                    let mut op = Atom::new(Class::Op, Nucleus::LargeOp(c));
                    op.limits = limits;
                    return Ok(Noad::Atom(op));
                }
                if let Some(&(_, limits)) = OPERATOR_NAMES.iter().find(|op| op.0 == name) {
                    let mut op = Atom::new(Class::Op, Nucleus::Text(name.into(), Font::Math));
                    op.limits = limits;
                    return Ok(Noad::Atom(op));
                }
                match symbol(name) {
                    Some((c, class)) => atom(class, Nucleus::Char(c)),
                    None => Err(format!("`\\{name}` is not a command acre knows")),
                }
            }
        }
    }
}

/// Put `script` on the last atom of `list` as its superscript, or its
/// subscript, adding an empty atom when the list does not end in one.
fn attach(list: &mut Vec<Noad>, sup: bool, script: Vec<Noad>) -> Result<(), String> {
    if !matches!(list.last(), Some(Noad::Atom(_))) {
        list.push(Noad::Atom(Atom::new(Class::Ord, Nucleus::Empty)));
    }
    let Some(Noad::Atom(atom)) = list.last_mut() else {
        unreachable!("an atom was just pushed");
    };
    let slot = if sup { &mut atom.sup } else { &mut atom.sub };
    if slot.is_some() {
        let which = if sup { "superscript" } else { "subscript" };
        return Err(format!(
            "a second {which} on one atom; group the first, as in `{{x^a}}^b`"
        ));
    }
    *slot = Some(script);
    Ok(())
}

/// The atom a character typed as it is stands for. Latin letters and
/// lowercase Greek are set in italic.
fn char_noad(c: char) -> Noad {
    if c == '~' {
        return Noad::Space(6.0);
    }
    let (c, class) = match c {
        '+' => ('+', Class::Bin),
        '-' | '\u{2212}' => ('\u{2212}', Class::Bin),
        '*' => ('\u{2217}', Class::Bin),
        '=' | '<' | '>' | ':' => (c, Class::Rel),
        ',' | ';' => (c, Class::Punct),
        '(' | '[' => (c, Class::Open),
        ')' | ']' | '!' | '?' => (c, Class::Close),
        '\'' => ('\u{2032}', Class::Ord),
        c => match SYMBOLS.iter().find(|s| s.1 == c) {
            Some(&(_, c, class)) => (c, class),
            None => (italic(c), Class::Ord),
        },
    };
    Noad::Atom(Atom::new(class, Nucleus::Char(c)))
}

/// `c` in math italic when it is a Latin letter or lowercase Greek.
fn italic(c: char) -> char {
    let shift = |base: u32, from: char| char::from_u32(base + (c as u32 - from as u32));
    let it = match c {
        // the italic h sits outside the block, in letterlike symbols
        'h' => Some('\u{210e}'),
        'A'..='Z' => shift(0x1d434, 'A'),
        'a'..='z' => shift(0x1d44e, 'a'),
        '\u{3b1}'..='\u{3c9}' => shift(0x1d6fc, '\u{3b1}'),
        '\u{2202}' => Some('\u{1d715}'),
        '\u{3f5}' => Some('\u{1d716}'),
        '\u{3d1}' => Some('\u{1d717}'),
        '\u{3d5}' => Some('\u{1d719}'),
        '\u{3f1}' => Some('\u{1d71a}'),
        '\u{3d6}' => Some('\u{1d71b}'),
        _ => None,
    };
    it.unwrap_or(c)
}

fn is_delimiter(c: char) -> bool {
    matches!(
        c,
        '(' | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '|'
            | '/'
            | '\\'
            | '\u{2016}'
            | '\u{2308}'
            | '\u{2309}'
            | '\u{230a}'
            | '\u{230b}'
            | '\u{27e8}'
            | '\u{27e9}'
            | '\u{2191}'
            | '\u{2193}'
    )
}

/// The character and class of the symbol command `\name`.
fn symbol(name: &str) -> Option<(char, Class)> {
    if let Some(&(_, c)) = GREEK.iter().find(|g| g.0 == name) {
        return Some((italic(c), Class::Ord));
    }
    SYMBOLS
        .iter()
        .find(|s| s.0 == name)
        .map(|&(_, c, class)| (c, class))
}

/// Greek letters by name. Lowercase is set in italic and uppercase
/// upright.
const GREEK: &[(&str, char)] = &[
    ("alpha", 'α'),
    ("beta", 'β'),
    ("gamma", 'γ'),
    ("delta", 'δ'),
    ("epsilon", 'ϵ'),
    ("varepsilon", 'ε'),
    ("zeta", 'ζ'),
    ("eta", 'η'),
    ("theta", 'θ'),
    ("vartheta", 'ϑ'),
    ("iota", 'ι'),
    ("kappa", 'κ'),
    ("lambda", 'λ'),
    ("mu", 'μ'),
    ("nu", 'ν'),
    ("xi", 'ξ'),
    ("pi", 'π'),
    ("varpi", 'ϖ'),
    ("rho", 'ρ'),
    ("varrho", 'ϱ'),
    ("sigma", 'σ'),
    ("varsigma", 'ς'),
    ("tau", 'τ'),
    ("upsilon", 'υ'),
    ("phi", 'ϕ'),
    ("varphi", 'φ'),
    ("chi", 'χ'),
    ("psi", 'ψ'),
    ("omega", 'ω'),
    ("Gamma", 'Γ'),
    ("Delta", 'Δ'),
    ("Theta", 'Θ'),
    ("Lambda", 'Λ'),
    ("Xi", 'Ξ'),
    ("Pi", 'Π'),
    ("Sigma", 'Σ'),
    ("Upsilon", 'Υ'),
    ("Phi", 'Φ'),
    ("Psi", 'Ψ'),
    ("Omega", 'Ω'),
];

/// Symbol commands, and the class of each. A character typed as it is
/// takes its class from here too.
const SYMBOLS: &[(&str, char, Class)] = &[
    // relations
    ("le", '≤', Class::Rel),
    ("leq", '≤', Class::Rel),
    ("ge", '≥', Class::Rel),
    ("geq", '≥', Class::Rel),
    ("ne", '≠', Class::Rel),
    ("neq", '≠', Class::Rel),
    ("approx", '≈', Class::Rel),
    ("equiv", '≡', Class::Rel),
    ("sim", '∼', Class::Rel),
    ("simeq", '≃', Class::Rel),
    ("cong", '≅', Class::Rel),
    ("propto", '∝', Class::Rel),
    ("ll", '≪', Class::Rel),
    ("gg", '≫', Class::Rel),
    ("in", '∈', Class::Rel),
    ("notin", '∉', Class::Rel),
    ("ni", '∋', Class::Rel),
    ("subset", '⊂', Class::Rel),
    ("subseteq", '⊆', Class::Rel),
    ("supset", '⊃', Class::Rel),
    ("supseteq", '⊇', Class::Rel),
    ("mid", '∣', Class::Rel),
    ("parallel", '∥', Class::Rel),
    ("perp", '⊥', Class::Rel),
    ("prec", '≺', Class::Rel),
    ("succ", '≻', Class::Rel),
    ("to", '→', Class::Rel),
    ("rightarrow", '→', Class::Rel),
    ("leftarrow", '←', Class::Rel),
    ("gets", '←', Class::Rel),
    ("leftrightarrow", '↔', Class::Rel),
    ("Rightarrow", '⇒', Class::Rel),
    ("Leftarrow", '⇐', Class::Rel),
    ("Leftrightarrow", '⇔', Class::Rel),
    ("longrightarrow", '⟶', Class::Rel),
    ("longleftarrow", '⟵', Class::Rel),
    ("implies", '⟹', Class::Rel),
    ("iff", '⟺', Class::Rel),
    ("mapsto", '↦', Class::Rel),
    ("uparrow", '↑', Class::Rel),
    ("downarrow", '↓', Class::Rel),
    // binary operators
    ("pm", '±', Class::Bin),
    ("mp", '∓', Class::Bin),
    ("times", '×', Class::Bin),
    ("cdot", '⋅', Class::Bin),
    ("div", '÷', Class::Bin),
    ("ast", '∗', Class::Bin),
    ("circ", '∘', Class::Bin),
    ("bullet", '∙', Class::Bin),
    ("cup", '∪', Class::Bin),
    ("cap", '∩', Class::Bin),
    ("wedge", '∧', Class::Bin),
    ("land", '∧', Class::Bin),
    ("vee", '∨', Class::Bin),
    ("lor", '∨', Class::Bin),
    ("setminus", '∖', Class::Bin),
    ("oplus", '⊕', Class::Bin),
    ("otimes", '⊗', Class::Bin),
    ("star", '⋆', Class::Bin),
    // ordinary symbols
    ("infty", '∞', Class::Ord),
    ("partial", '∂', Class::Ord),
    ("nabla", '∇', Class::Ord),
    ("forall", '∀', Class::Ord),
    ("exists", '∃', Class::Ord),
    ("neg", '¬', Class::Ord),
    ("lnot", '¬', Class::Ord),
    ("emptyset", '∅', Class::Ord),
    ("varnothing", '∅', Class::Ord),
    ("ell", 'ℓ', Class::Ord),
    ("hbar", 'ℏ', Class::Ord),
    ("prime", '′', Class::Ord),
    ("dots", '…', Class::Ord),
    ("ldots", '…', Class::Ord),
    ("cdots", '⋯', Class::Ord),
    ("vdots", '⋮', Class::Ord),
    ("ddots", '⋱', Class::Ord),
    ("angle", '∠', Class::Ord),
    ("top", '⊤', Class::Ord),
    ("bot", '⊥', Class::Ord),
    ("aleph", 'ℵ', Class::Ord),
    ("Re", 'ℜ', Class::Ord),
    ("Im", 'ℑ', Class::Ord),
    ("|", '‖', Class::Ord),
    ("backslash", '\\', Class::Ord),
    ("vert", '|', Class::Ord),
    ("Vert", '‖', Class::Ord),
    ("$", '$', Class::Ord),
    ("%", '%', Class::Ord),
    ("#", '#', Class::Ord),
    ("&", '&', Class::Ord),
    ("_", '_', Class::Ord),
    ("/", '/', Class::Ord),
    // delimiters
    ("{", '{', Class::Open),
    ("}", '}', Class::Close),
    ("lbrace", '{', Class::Open),
    ("rbrace", '}', Class::Close),
    ("langle", '⟨', Class::Open),
    ("rangle", '⟩', Class::Close),
    ("lfloor", '⌊', Class::Open),
    ("rfloor", '⌋', Class::Close),
    ("lceil", '⌈', Class::Open),
    ("rceil", '⌉', Class::Close),
    ("lvert", '|', Class::Open),
    ("rvert", '|', Class::Close),
    ("lVert", '‖', Class::Open),
    ("rVert", '‖', Class::Close),
    // punctuation
    ("colon", ':', Class::Punct),
];

/// Large operators: the name, the character, and whether its scripts go
/// above and below in display style.
const LARGE_OPS: &[(&str, char, bool)] = &[
    ("sum", '∑', true),
    ("prod", '∏', true),
    ("coprod", '∐', true),
    ("bigcup", '⋃', true),
    ("bigcap", '⋂', true),
    ("bigvee", '⋁', true),
    ("bigwedge", '⋀', true),
    ("bigoplus", '⨁', true),
    ("bigotimes", '⨂', true),
    ("int", '∫', false),
    ("iint", '∬', false),
    ("oint", '∮', false),
];

/// Operator names, set upright, and whether their scripts go above and
/// below in display style.
const OPERATOR_NAMES: &[(&str, bool)] = &[
    ("log", false),
    ("ln", false),
    ("lg", false),
    ("exp", false),
    ("sin", false),
    ("cos", false),
    ("tan", false),
    ("sec", false),
    ("csc", false),
    ("cot", false),
    ("arcsin", false),
    ("arccos", false),
    ("arctan", false),
    ("sinh", false),
    ("cosh", false),
    ("tanh", false),
    ("arg", false),
    ("deg", false),
    ("dim", false),
    ("hom", false),
    ("ker", false),
    ("det", true),
    ("gcd", true),
    ("Pr", true),
    ("lim", true),
    ("liminf", true),
    ("limsup", true),
    ("max", true),
    ("min", true),
    ("sup", true),
    ("inf", true),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn atoms(src: &str) -> Vec<Atom> {
        parse(src)
            .unwrap()
            .into_iter()
            .map(|n| match n {
                Noad::Atom(a) => a,
                other => panic!("expected an atom, got {other:?}"),
            })
            .collect()
    }

    fn chars(src: &str) -> Vec<(char, Class)> {
        atoms(src)
            .into_iter()
            .map(|a| match a.nucleus {
                Nucleus::Char(c) => (c, a.class),
                other => panic!("expected a character, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn letters_are_italic_and_symbols_take_their_class() {
        assert_eq!(
            chars(r"x - 2 \le \alpha \Gamma"),
            [
                ('𝑥', Class::Ord),
                ('−', Class::Bin),
                ('2', Class::Ord),
                ('≤', Class::Rel),
                ('𝛼', Class::Ord),
                ('Γ', Class::Ord),
            ]
        );
        assert_eq!(chars("h")[0].0, 'ℎ');
    }

    #[test]
    fn scripts_attach_to_the_atom_before_them_or_to_an_empty_one() {
        let a = &atoms("x_i^{2k}")[0];
        assert_eq!(a.sub.as_ref().unwrap().len(), 1);
        assert_eq!(a.sup.as_ref().unwrap().len(), 2);
        let a = &atoms("{x^a}^b")[0];
        assert!(matches!(&a.nucleus, Nucleus::Group(g) if g.len() == 1));
        assert!(a.sup.is_some());
        let a = &atoms("^2")[0];
        assert_eq!(a.nucleus, Nucleus::Empty);
        assert!(a.sup.is_some());
    }

    #[test]
    fn commands_take_braced_or_single_arguments() {
        let [frac, sqrt] = &atoms(r"\frac12 \sqrt[3]{x+1}")[..] else {
            panic!("expected two atoms");
        };
        assert!(matches!(&frac.nucleus, Nucleus::Frac(n, d) if n.len() == 1 && d.len() == 1));
        match &sqrt.nucleus {
            Nucleus::Sqrt { index, radicand } => {
                assert_eq!(index.as_ref().unwrap().len(), 1);
                assert_eq!(radicand.len(), 3);
            }
            other => panic!("expected a radical, got {other:?}"),
        }
    }

    #[test]
    fn left_and_right_fence_a_list_and_dot_is_no_delimiter() {
        let a = &atoms(r"\left\lceil x \right.")[0];
        assert_eq!(a.class, Class::Inner);
        assert_eq!(
            a.nucleus,
            Nucleus::Fenced {
                left: Some('⌈'),
                body: parse("x").unwrap(),
                right: None,
            }
        );
    }

    #[test]
    fn operators_know_whether_they_take_limits() {
        let ops = atoms(r"\sum \int \log \lim \int\limits \sum\nolimits");
        let limits: Vec<bool> = ops.iter().map(|a| a.limits).collect();
        assert_eq!(limits, [true, false, false, true, true, false]);
        assert!(ops.iter().all(|a| a.class == Class::Op));
        assert_eq!(ops[2].nucleus, Nucleus::Text("log".into(), Font::Math));
    }

    #[test]
    fn text_keeps_its_spaces_and_mathrm_drops_them() {
        let [text, rm] = &atoms(r"\text{for all} \mathrm{d x}")[..] else {
            panic!("expected two atoms");
        };
        assert_eq!(text.nucleus, Nucleus::Text("for all".into(), Font::Sans));
        assert_eq!(rm.nucleus, Nucleus::Text("dx".into(), Font::Math));
    }

    #[test]
    fn styles_and_spaces_are_items_of_the_list() {
        assert_eq!(
            parse(r"\displaystyle \, \quad ~").unwrap(),
            [
                Noad::Style(Style::Display),
                Noad::Space(3.0),
                Noad::Space(18.0),
                Noad::Space(6.0),
            ]
        );
    }

    #[test]
    fn mistakes_say_what_is_wrong() {
        let err = |src| parse(src).unwrap_err();
        assert_eq!(err(r"\foo"), "`\\foo` is not a command acre knows");
        assert!(err("x^1^2").starts_with("a second superscript"));
        assert_eq!(err("{x"), "a `{` is never closed");
        assert_eq!(err("x}"), "a `}` closes no `{`");
        assert_eq!(err(r"\left( x"), "a `\\left` has no `\\right`");
        assert_eq!(err(r"x \right)"), "a `\\right` has no `\\left`");
        assert_eq!(err(r"\left x \right)"), "`x` is not a delimiter");
        assert_eq!(
            err(r"\frac{a}"),
            "an argument is missing at the end of the formula"
        );
        assert_eq!(err(r"x \limits"), "`\\limits` follows no operator");
    }
}
