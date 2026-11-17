//! Catalogue and input state for the horizontal tool chooser. No window,
//! renderer, or platform handles are needed to filter or edit a search query.

use crate::document::{BrushKind, ShapeKind};
use crate::i18n::Language;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Brush,
    Eraser,
    Shapes,
}

impl Family {
    pub const ALL: [Self; 3] = [Self::Brush, Self::Eraser, Self::Shapes];

    pub const fn index(self) -> usize {
        match self {
            Self::Brush => 0,
            Self::Eraser => 1,
            Self::Shapes => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemId {
    Brush(BrushKind),
    Eraser(BrushKind),
    Shape(ShapeKind),
}

impl ItemId {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Brush(BrushKind::Ink) => "Тушь",
            Self::Brush(BrushKind::Soft) => "Мягкая",
            Self::Brush(BrushKind::Marker) => "Маркер",
            Self::Brush(BrushKind::Pencil) => "Карандаш",
            Self::Brush(BrushKind::Charcoal) => "Уголь",
            Self::Brush(BrushKind::Chalk) => "Мел",
            Self::Brush(BrushKind::Airbrush) => "Аэрограф",
            Self::Brush(BrushKind::Calligraphy) => "Каллиграфия",
            Self::Brush(BrushKind::Spray) => "Брызги",
            Self::Brush(BrushKind::Flat) => "Плоская",
            Self::Eraser(BrushKind::Ink) => "Твёрдый",
            Self::Eraser(BrushKind::Soft) => "Мягкий",
            Self::Eraser(BrushKind::Marker) => "Плоский",
            Self::Eraser(BrushKind::Pencil) => "Точный",
            Self::Eraser(BrushKind::Charcoal) => "Фактурный",
            Self::Eraser(BrushKind::Chalk) => "Зернистый",
            Self::Eraser(BrushKind::Airbrush) => "Воздушный",
            Self::Eraser(BrushKind::Calligraphy) => "Скошенный",
            Self::Eraser(BrushKind::Spray) => "Крапчатый",
            Self::Eraser(BrushKind::Flat) => "Широкий",
            Self::Shape(ShapeKind::Line) => "Линия",
            Self::Shape(ShapeKind::Rectangle) => "Прямоугольник",
            Self::Shape(ShapeKind::Ellipse) => "Эллипс",
        }
    }

    const fn aliases(self) -> &'static str {
        match self {
            Self::Brush(BrushKind::Ink) => "ink inking pen",
            Self::Brush(BrushKind::Soft) => "soft round smooth painting",
            Self::Brush(BrushKind::Marker) => "marker felt pen paint",
            Self::Brush(BrushKind::Pencil) => "pencil graphite sketch",
            Self::Brush(BrushKind::Charcoal) => "charcoal coal sketch texture",
            Self::Brush(BrushKind::Chalk) => "chalk pastel sketch texture",
            Self::Brush(BrushKind::Airbrush) => "airbrush air brush soft spray",
            Self::Brush(BrushKind::Calligraphy) => "calligraphy nib lettering ink",
            Self::Brush(BrushKind::Spray) => "spray splatter splashes texture",
            Self::Brush(BrushKind::Flat) => "flat paint painting brush",
            Self::Eraser(BrushKind::Ink) => "hard solid round erase eraser rubber",
            Self::Eraser(BrushKind::Soft) => "soft feather smooth erase eraser rubber",
            Self::Eraser(BrushKind::Marker) => "flat chisel erase eraser rubber",
            Self::Eraser(BrushKind::Pencil) => "precise precision fine detail erase eraser rubber",
            Self::Eraser(BrushKind::Charcoal) => "textured rough erase eraser rubber",
            Self::Eraser(BrushKind::Chalk) => "grainy grain granular erase eraser rubber",
            Self::Eraser(BrushKind::Airbrush) => "airy air feather fade erase eraser rubber",
            Self::Eraser(BrushKind::Calligraphy) => {
                "angled slanted bevel chisel erase eraser rubber"
            }
            Self::Eraser(BrushKind::Spray) => "speckled speckle dots erase eraser rubber",
            Self::Eraser(BrushKind::Flat) => "wide broad erase eraser rubber",
            Self::Shape(ShapeKind::Line) => "line straight segment",
            Self::Shape(ShapeKind::Rectangle) => "rectangle rect square box",
            Self::Shape(ShapeKind::Ellipse) => "ellipse oval circle",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Category {
    #[default]
    All,
    Ink,
    Sketch,
    Painting,
    Soft,
    Texture,
    Spray,
    Calligraphy,
    Flat,
    Line,
    Angular,
    Curves,
}

impl Category {
    pub const fn for_family(family: Family) -> &'static [Self] {
        match family {
            Family::Brush | Family::Eraser => &[
                Self::All,
                Self::Ink,
                Self::Sketch,
                Self::Painting,
                Self::Soft,
                Self::Texture,
                Self::Spray,
                Self::Calligraphy,
                Self::Flat,
            ],
            Family::Shapes => &[Self::All, Self::Line, Self::Angular, Self::Curves],
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "Все",
            Self::Ink => "Тушь",
            Self::Sketch => "Скетч",
            Self::Painting => "Живопись",
            Self::Soft => "Мягкие",
            Self::Texture => "Фактура",
            Self::Spray => "Распыление",
            Self::Calligraphy => "Каллиграфия",
            Self::Flat => "Плоские",
            Self::Line => "Линии",
            Self::Angular => "Углы",
            Self::Curves => "Кривые",
        }
    }

    pub const fn icon(self) -> &'static str {
        match self {
            Self::All => "grid",
            Self::Ink => "nib",
            Self::Sketch => "pencil",
            Self::Painting => "brush",
            Self::Soft => "soft",
            Self::Texture => "texture",
            Self::Spray => "spray",
            Self::Calligraphy => "calligraphy",
            Self::Flat => "flat",
            Self::Line => "line",
            Self::Angular => "rect",
            Self::Curves => "ellipse",
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Ink => "ink",
            Self::Sketch => "sketch",
            Self::Painting => "painting",
            Self::Soft => "soft",
            Self::Texture => "texture",
            Self::Spray => "spray",
            Self::Calligraphy => "calligraphy",
            Self::Flat => "flat",
            Self::Line => "line",
            Self::Angular => "angular",
            Self::Curves => "curves",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let key = key.trim().to_ascii_lowercase();
        Self::for_family(Family::Brush)
            .iter()
            .chain(Self::for_family(Family::Shapes))
            .copied()
            .find(|category| category.key() == key)
    }

    fn contains(self, item: ItemId) -> bool {
        use BrushKind as B;
        use ShapeKind as S;
        // Erasers use the corresponding footprint categories while retaining
        // their own catalogue identity and labels.
        let item = match item {
            ItemId::Eraser(kind) => ItemId::Brush(kind),
            other => other,
        };
        match self {
            Self::All => true,
            Self::Ink => matches!(item, ItemId::Brush(B::Ink | B::Marker | B::Calligraphy)),
            Self::Sketch => matches!(item, ItemId::Brush(B::Pencil | B::Charcoal | B::Chalk)),
            Self::Painting => matches!(item, ItemId::Brush(B::Soft | B::Marker | B::Flat)),
            Self::Soft => matches!(item, ItemId::Brush(B::Soft | B::Airbrush)),
            Self::Texture => matches!(item, ItemId::Brush(B::Charcoal | B::Chalk | B::Spray)),
            Self::Spray => matches!(item, ItemId::Brush(B::Airbrush | B::Spray)),
            Self::Calligraphy => matches!(item, ItemId::Brush(B::Calligraphy)),
            Self::Flat => matches!(item, ItemId::Brush(B::Marker | B::Flat)),
            Self::Line => matches!(item, ItemId::Shape(S::Line)),
            Self::Angular => matches!(item, ItemId::Shape(S::Rectangle)),
            Self::Curves => matches!(item, ItemId::Shape(S::Ellipse)),
        }
    }
}

/// A finite, clamped logical-pixel offset. Each chooser owns independent
/// instances for its item strip and category strip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scroll {
    pub offset: f32,
    pub extent: f32,
    pub viewport: f32,
}

impl Scroll {
    pub fn max(&self) -> f32 {
        if self.extent.is_finite() && self.viewport.is_finite() {
            (self.extent.max(0.) - self.viewport.max(0.)).max(0.)
        } else {
            0.
        }
    }

    pub fn set_extent(&mut self, content: f32, viewport: f32) -> bool {
        let previous = *self;
        self.extent = if content.is_finite() {
            content.max(0.)
        } else {
            0.
        };
        self.viewport = if viewport.is_finite() {
            viewport.max(0.)
        } else {
            0.
        };
        self.offset = if self.offset.is_finite() {
            self.offset.clamp(0., self.max())
        } else {
            0.
        };
        *self != previous
    }

    pub fn set(&mut self, offset: f32) -> bool {
        if !offset.is_finite() {
            return false;
        }
        let next = offset.clamp(0., self.max());
        let changed = next != self.offset;
        self.offset = next;
        changed
    }

    pub fn scroll_by(&mut self, delta: f32) -> bool {
        if !delta.is_finite() {
            return false;
        }
        // Use f64 for the addition so a large finite delta still reaches the
        // corresponding edge instead of overflowing to a rejected infinity.
        let next = (self.offset as f64 + delta as f64).clamp(0., self.max() as f64);
        self.set(next as f32)
    }

    pub fn ensure_visible(&mut self, start: f32, end: f32) -> bool {
        if !start.is_finite() || !end.is_finite() || end < start {
            return false;
        }
        if start < self.offset || end - start > self.viewport {
            self.set(start)
        } else if end > self.offset + self.viewport {
            self.set(end - self.viewport)
        } else {
            false
        }
    }
}

/// UTF-8 byte positions are always on scalar boundaries. The limit counts
/// Unicode scalar values, not bytes, and pasted content remains one line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchText {
    text: String,
    caret: usize,
    anchor: usize,
}

impl SearchText {
    pub const MAX_SCALARS: usize = 128;

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        (self.caret != self.anchor)
            .then_some((self.caret.min(self.anchor), self.caret.max(self.anchor)))
    }

    pub fn selected_text(&self) -> String {
        self.selection()
            .map(|(start, end)| self.text[start..end].to_owned())
            .unwrap_or_default()
    }

    pub fn insert(&mut self, value: &str) -> bool {
        let (start, end) = self.selection().unwrap_or((self.caret, self.caret));
        let retained = self.text.chars().count() - self.text[start..end].chars().count();
        let inserted: String = value
            .chars()
            .filter_map(|c| {
                if c.is_whitespace() {
                    Some(' ')
                } else if c.is_control() {
                    None
                } else {
                    Some(c)
                }
            })
            .take(Self::MAX_SCALARS.saturating_sub(retained))
            .collect();
        // Empty or control-only input is not an implicit deletion command.
        if inserted.is_empty() {
            return false;
        }
        let changed = self.text[start..end] != inserted;
        self.text.replace_range(start..end, &inserted);
        self.caret = start + inserted.len();
        self.anchor = self.caret;
        changed
    }

    pub fn clear(&mut self) -> bool {
        let changed = !self.text.is_empty();
        self.text.clear();
        self.caret = 0;
        self.anchor = 0;
        changed
    }

    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            return false;
        };
        self.text.replace_range(start..end, "");
        self.caret = start;
        self.anchor = start;
        true
    }

    pub fn backspace(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.caret == 0 {
            return false;
        }
        let start = self.previous_boundary(self.caret);
        self.text.replace_range(start..self.caret, "");
        self.caret = start;
        self.anchor = start;
        true
    }

    pub fn delete(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.caret == self.text.len() {
            return false;
        }
        let end = self.next_boundary(self.caret);
        self.text.replace_range(self.caret..end, "");
        self.anchor = self.caret;
        true
    }

    pub fn move_left(&mut self, extend: bool, word: bool) -> bool {
        if !extend {
            if let Some((start, _)) = self.selection() {
                return self.move_to(start, false);
            }
        }
        let mut next = self.previous_boundary(self.caret);
        if word {
            next = self.caret;
            while next > 0 && !word_char(self.text[..next].chars().next_back().unwrap()) {
                next = self.previous_boundary(next);
            }
            while next > 0 && word_char(self.text[..next].chars().next_back().unwrap()) {
                next = self.previous_boundary(next);
            }
        }
        self.move_to(next, extend)
    }

    pub fn move_right(&mut self, extend: bool, word: bool) -> bool {
        if !extend {
            if let Some((_, end)) = self.selection() {
                return self.move_to(end, false);
            }
        }
        let mut next = self.next_boundary(self.caret);
        if word {
            next = self.caret;
            while next < self.text.len() && word_char(self.text[next..].chars().next().unwrap()) {
                next = self.next_boundary(next);
            }
            while next < self.text.len() && !word_char(self.text[next..].chars().next().unwrap()) {
                next = self.next_boundary(next);
            }
        }
        self.move_to(next, extend)
    }

    pub fn home(&mut self, extend: bool) -> bool {
        self.move_to(0, extend)
    }

    pub fn end(&mut self, extend: bool) -> bool {
        self.move_to(self.text.len(), extend)
    }

    pub fn select_all(&mut self) -> bool {
        let changed = self.anchor != 0 || self.caret != self.text.len();
        self.anchor = 0;
        self.caret = self.text.len();
        changed
    }

    fn previous_boundary(&self, from: usize) -> usize {
        self.text[..from]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self, from: usize) -> usize {
        from + self.text[from..].chars().next().map_or(0, char::len_utf8)
    }

    fn move_to(&mut self, byte: usize, extend: bool) -> bool {
        let previous = (self.caret, self.anchor);
        self.caret = byte;
        if !extend {
            self.anchor = byte;
        }
        previous != (self.caret, self.anchor)
    }
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub query: SearchText,
    pub search_open: bool,
    pub category: Category,
    pub items: Scroll,
    pub categories: Scroll,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Browser {
    views: [View; 3],
}

impl Browser {
    pub fn view(&self, family: Family) -> &View {
        &self.views[family.index()]
    }

    pub fn view_mut(&mut self, family: Family) -> &mut View {
        &mut self.views[family.index()]
    }

    pub fn set_category(&mut self, family: Family, category: Category) -> bool {
        if !Category::for_family(family).contains(&category) {
            return false;
        }
        let view = self.view_mut(family);
        if view.category == category {
            return false;
        }
        view.category = category;
        view.items.set(0.);
        true
    }

    pub fn set_query(&mut self, family: Family, value: &str) -> bool {
        let view = self.view_mut(family);
        let mut query = SearchText::default();
        query.insert(value);
        let changed = query.text() != view.query.text();
        view.query = query;
        if changed {
            view.items.set(0.);
        }
        changed
    }

    /// Call after an edit through view_mut(...).query returned true. The
    /// independently scrolled category strip keeps its position.
    pub fn query_changed(&mut self, family: Family) {
        self.view_mut(family).items.set(0.);
    }

    pub fn filtered_ids(&self, family: Family, language: Language) -> Vec<ItemId> {
        let view = self.view(family);
        let query = fold(view.query.text());
        let tokens: Vec<_> = query.split_whitespace().collect();
        let ids: Vec<_> = match family {
            Family::Brush => BrushKind::ALL.into_iter().map(ItemId::Brush).collect(),
            Family::Eraser => BrushKind::ALL.into_iter().map(ItemId::Eraser).collect(),
            Family::Shapes => [ShapeKind::Line, ShapeKind::Rectangle, ShapeKind::Ellipse]
                .into_iter()
                .map(ItemId::Shape)
                .collect(),
        };
        ids.into_iter()
            .filter(|&id| {
                if !view.category.contains(id) {
                    return false;
                }
                if tokens.is_empty() {
                    return true;
                }
                let mut names = fold(&format!(
                    "{} {} {}",
                    id.label(),
                    language.translate(id.label()),
                    id.aliases()
                ));
                if let ItemId::Eraser(kind) = id {
                    // Preserve searches saved or remembered from the shared
                    // brush catalogue, including its current-locale names.
                    let legacy = ItemId::Brush(kind);
                    names.push(' ');
                    names.push_str(&fold(&format!(
                        "{} {} {}",
                        legacy.label(),
                        language.translate(legacy.label()),
                        legacy.aliases()
                    )));
                }
                tokens.iter().all(|token| names.contains(token))
            })
            .collect()
    }
}

fn fold(value: &str) -> String {
    // Unicode lowercase plus the common case-fold expansions which lowercase
    // alone misses. Search remains dependency-free and preserves the raw query.
    value
        .chars()
        .flat_map(char::to_lowercase)
        .fold(String::new(), |mut out, c| {
            match c {
                'ß' => out.push_str("ss"),
                'ς' => out.push('σ'),
                'ﬀ' => out.push_str("ff"),
                'ﬁ' => out.push_str("fi"),
                'ﬂ' => out.push_str("fl"),
                'ﬃ' => out.push_str("ffi"),
                'ﬄ' => out.push_str("ffl"),
                'ﬅ' | 'ﬆ' => out.push_str("st"),
                _ => out.push(c),
            }
            out
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_edits_utf8_without_splitting_scalars() {
        let mut text = SearchText::default();
        assert!(text.insert("Кисть 한字🙂"));
        assert_eq!(text.caret(), text.text().len());
        text.move_left(true, false);
        assert_eq!(text.selected_text(), "🙂");
        assert!(text.insert("絵"));
        assert_eq!(text.text(), "Кисть 한字絵");
        assert!(text.backspace());
        text.home(false);
        assert!(text.delete());
        assert_eq!(text.text(), "исть 한字");
        text.select_all();
        assert!(text.insert("a\r\n한\t字\0\u{2028}z"));
        assert_eq!(text.text(), "a  한 字 z");
        assert!(text.text().is_char_boundary(text.caret()));
        assert_eq!(text.selection(), None);
    }

    #[test]
    fn search_limit_counts_scalars_and_selection_frees_capacity() {
        let mut text = SearchText::default();
        assert!(text.insert(&"🙂".repeat(200)));
        assert_eq!(text.text().chars().count(), 128);
        assert_eq!(text.text().len(), 512);
        assert!(!text.insert("字"));
        text.move_left(true, false);
        assert!(text.insert("한字"));
        assert_eq!(text.text().chars().count(), 128);
        assert!(text.text().ends_with('한'));
        text.select_all();
        assert!(!text.insert("\0"));
        assert_eq!(text.selected_text().chars().count(), 128);
        assert!(text.delete());
        assert!(text.text().is_empty());
    }

    #[test]
    fn word_navigation_and_shift_selection_use_safe_byte_offsets() {
        let mut text = SearchText::default();
        text.insert("soft-кисть 日本語");
        text.home(false);
        text.move_right(false, true);
        assert_eq!(&text.text()[text.caret()..], "кисть 日本語");
        text.move_right(true, true);
        assert_eq!(text.selected_text(), "кисть ");
        text.move_left(false, false);
        assert_eq!(text.selection(), None);
        assert_eq!(&text.text()[text.caret()..], "кисть 日本語");
        text.end(false);
        text.move_left(true, true);
        assert_eq!(text.selected_text(), "日本語");
    }

    #[test]
    fn filtering_combines_category_tokens_and_current_locale() {
        let mut browser = Browser::default();
        assert_eq!(
            browser.filtered_ids(Family::Brush, Language::Russian).len(),
            10
        );
        assert_eq!(
            browser
                .filtered_ids(Family::Shapes, Language::English)
                .len(),
            3
        );
        browser.set_category(Family::Brush, Category::Ink);
        browser.set_query(Family::Brush, "mArKeR PEN");
        assert_eq!(
            browser.filtered_ids(Family::Brush, Language::Russian),
            vec![ItemId::Brush(BrushKind::Marker)]
        );
        browser.set_category(Family::Brush, Category::Sketch);
        assert!(browser
            .filtered_ids(Family::Brush, Language::Russian)
            .is_empty());
        browser.set_query(
            Family::Brush,
            &Language::German.translate("Уголь").to_uppercase(),
        );
        assert_eq!(
            browser.filtered_ids(Family::Brush, Language::German),
            vec![ItemId::Brush(BrushKind::Charcoal)]
        );
        browser.set_query(Family::Shapes, "несуществующий вариант");
        assert!(browser
            .filtered_ids(Family::Shapes, Language::Russian)
            .is_empty());
        browser.set_query(Family::Shapes, "CIRCLE");
        assert_eq!(
            browser.filtered_ids(Family::Shapes, Language::Japanese),
            vec![ItemId::Shape(ShapeKind::Ellipse)]
        );
        assert!(!browser.set_category(Family::Shapes, Category::Ink));
    }

    #[test]
    fn family_state_and_category_scroll_survive_filter_changes() {
        let mut browser = Browser::default();
        let view = browser.view_mut(Family::Brush);
        view.items.set_extent(1032., 408.);
        view.items.set(400.);
        view.categories.set_extent(320., 280.);
        view.categories.set(32.);
        assert!(browser.set_query(Family::Brush, "soft"));
        assert_eq!(browser.view(Family::Brush).items.offset, 0.);
        assert_eq!(browser.view(Family::Brush).categories.offset, 32.);
        browser.set_category(Family::Eraser, Category::Texture);
        browser.set_query(Family::Eraser, "chalk");
        assert_eq!(browser.view(Family::Brush).query.text(), "soft");
        assert_eq!(browser.view(Family::Eraser).category, Category::Texture);
        assert_eq!(browser.view(Family::Shapes).query.text(), "");
        browser.view_mut(Family::Brush).items.set(120.);
        assert!(!browser.set_category(Family::Brush, Category::All));
        assert_eq!(browser.view(Family::Brush).items.offset, 120.);
        browser.view_mut(Family::Brush).query.backspace();
        browser.query_changed(Family::Brush);
        assert_eq!(browser.view(Family::Brush).items.offset, 0.);
        assert_eq!(browser.view(Family::Brush).categories.offset, 32.);
    }

    #[test]
    fn scroll_clamps_drag_wheel_resize_and_visibility() {
        let mut scroll = Scroll::default();
        scroll.set_extent(352., 324.);
        assert_eq!(scroll.max(), 28.);
        assert!(scroll.ensure_visible(240., 352.));
        assert_eq!(scroll.offset, 28.);
        assert!(!scroll.scroll_by(200.));
        assert!(scroll.ensure_visible(0., 112.));
        assert_eq!(scroll.offset, 0.);
        scroll.scroll_by(f32::MAX);
        assert_eq!(scroll.offset, 28.);
        assert!(!scroll.set(f32::NAN));
        scroll.set_extent(80., 324.);
        assert_eq!(scroll.offset, 0.);
        assert_eq!(scroll.max(), 0.);
        scroll.set_extent(f32::INFINITY, 10.);
        assert_eq!(scroll.extent, 0.);
        assert!(scroll.offset.is_finite());
    }
}
