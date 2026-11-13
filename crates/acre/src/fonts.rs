//! The fonts a drawing is set in: the four text faces and the math font
//! acre embeds, and any a file registers with `fonts`.
//!
//! `text` measures with a [`Fonts`], `math` sets formulas from its math
//! font, and `render` draws from it, so all three agree on every width.
//! Nothing here reads a file. A program crate reads the bytes.

use std::sync::{Arc, LazyLock};

use rustybuzz::Face;
use self_cell::self_cell;

use crate::math::MathFont;
use crate::scene::Font;

static MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMonoNL-Regular.ttf");
static SANS: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
static SANS_BOLD: &[u8] = include_bytes!("../assets/fonts/NotoSans-Bold.ttf");
static SANS_ITALIC: &[u8] = include_bytes!("../assets/fonts/NotoSans-Italic.ttf");
static MATH: &[u8] = include_bytes!("../assets/fonts/NotoSansMath-Regular.ttf");

static EMBEDDED: LazyLock<Fonts> = LazyLock::new(|| {
    let face = |data: &'static [u8]| Typeface::parse(data).expect("the embedded font parses");
    Fonts(Arc::new(Set {
        text: vec![face(MONO), face(SANS), face(SANS_BOLD), face(SANS_ITALIC)],
        names: Vec::new(),
        math: MathFont::parse(MATH).expect("the embedded math font parses"),
    }))
});

// the face borrows the bytes the same cell owns
self_cell!(
    /// A font file and the face parsed from it.
    pub struct Typeface {
        owner: Arc<[u8]>,
        #[covariant]
        dependent: Face,
    }
);

impl Typeface {
    /// The face in the first font of `bytes`, or `None` when they are
    /// not a font.
    pub fn parse(bytes: impl Into<Arc<[u8]>>) -> Option<Typeface> {
        Typeface::try_new(bytes.into(), |b| Face::from_slice(b, 0).ok_or(())).ok()
    }

    pub fn face(&self) -> &Face<'_> {
        self.borrow_dependent()
    }

    /// The font file.
    pub fn bytes(&self) -> &Arc<[u8]> {
        self.borrow_owner()
    }
}

/// The fonts a scene is set in. A clone shares the fonts.
#[derive(Clone)]
pub struct Fonts(Arc<Set>);

struct Set {
    /// Mono, sans, sans bold and sans italic, then each registered font.
    text: Vec<Typeface>,
    /// The name of each registered font, in the order of `text` after
    /// the four embedded faces.
    names: Vec<String>,
    math: MathFont,
}

/// How many text faces acre embeds.
const EMBEDDED_TEXT: usize = 4;

impl Fonts {
    /// The embedded fonts with `custom` registered under their names,
    /// and `math` in place of the embedded math font when it is given.
    pub fn new(custom: Vec<(String, Typeface)>, math: Option<MathFont>) -> Fonts {
        let embedded = Fonts::default();
        if custom.is_empty() && math.is_none() {
            return embedded;
        }
        let mut text: Vec<Typeface> = embedded.0.text.iter().map(clone_typeface).collect();
        let mut names = Vec::new();
        for (name, typeface) in custom {
            names.push(name);
            text.push(typeface);
        }
        let math = math.unwrap_or_else(|| MathFont::parse(MATH).expect("the embedded math font"));
        Fonts(Arc::new(Set { text, names, math }))
    }

    /// The face `font` is set in.
    pub fn face(&self, font: Font) -> &Face<'_> {
        match font {
            Font::Math => self.0.math.face(),
            font => self.0.text[index(font)].face(),
        }
    }

    /// The font file `font` is set from.
    pub fn bytes(&self, font: Font) -> &Arc<[u8]> {
        match font {
            Font::Math => self.0.math.bytes(),
            font => self.0.text[index(font)].bytes(),
        }
    }

    pub fn math(&self) -> &MathFont {
        &self.0.math
    }

    /// The registered font called `name`.
    pub fn lookup(&self, name: &str) -> Option<Font> {
        let k = self.0.names.iter().position(|n| n == name)?;
        Some(Font::Custom(k as u16))
    }
}

fn index(font: Font) -> usize {
    match font {
        Font::Mono => 0,
        Font::Sans => 1,
        Font::SansBold => 2,
        Font::SansItalic => 3,
        Font::Custom(k) => EMBEDDED_TEXT + k as usize,
        Font::Math => unreachable!("the math font is not a text face"),
    }
}

fn clone_typeface(t: &Typeface) -> Typeface {
    Typeface::parse(t.bytes().clone()).expect("a parsed font parses again")
}

impl Default for Fonts {
    /// The embedded fonts.
    fn default() -> Fonts {
        EMBEDDED.clone()
    }
}

impl PartialEq for Fonts {
    fn eq(&self, other: &Fonts) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for Fonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fonts")
            .field("registered", &self.0.names)
            .finish()
    }
}
