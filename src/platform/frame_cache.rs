//! Small native scheduling/cache policies that can be verified without a HWND.
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::time::{Duration, Instant};

pub(crate) const FRAME_INTERVAL: Duration = Duration::from_millis(16);

pub(crate) struct FramePacer {
    dirty: bool,
    last_started: Instant,
}

impl FramePacer {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            dirty: true,
            last_started: now - FRAME_INTERVAL,
        }
    }
    pub(crate) fn request(&mut self) {
        self.dirty = true;
    }
    pub(crate) fn dirty(&self) -> bool {
        self.dirty
    }
    pub(crate) fn due(&self, now: Instant) -> bool {
        self.dirty && now.saturating_duration_since(self.last_started) >= FRAME_INTERVAL
    }
    pub(crate) fn begin_frame(&mut self, now: Instant) {
        // Measure cadence from frame start: an expensive render must not incur
        // an additional idle interval after it has already consumed the budget.
        self.dirty = false;
        self.last_started = now;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FontContext {
    pub pixels: i32,
    pub bold: bool,
    pub face: &'static str,
    pub script: &'static str,
}

pub(crate) struct FontChoiceCache<T> {
    contexts: HashMap<FontContext, HashMap<Rc<str>, T>>,
    order: VecDeque<(FontContext, Rc<str>)>,
}

impl<T> Default for FontChoiceCache<T> {
    fn default() -> Self {
        Self {
            contexts: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

impl<T: Copy> FontChoiceCache<T> {
    const MAX_ENTRIES: usize = 1024;
    const MAX_TEXT_BYTES: usize = 2048;

    pub(crate) fn get(&self, context: FontContext, text: &str) -> Option<T> {
        self.contexts.get(&context)?.get(text).copied()
    }
    pub(crate) fn insert(&mut self, context: FontContext, text: &str, value: T) {
        if text.len() > Self::MAX_TEXT_BYTES {
            return;
        }
        if let Some(entry) = self
            .contexts
            .get_mut(&context)
            .and_then(|map| map.get_mut(text))
        {
            *entry = value;
            return;
        }
        // FIFO eviction stays O(1) and allows new fonts/languages to warm up
        // even after many changing size/zoom labels have filled the cache.
        if self.order.len() == Self::MAX_ENTRIES {
            if let Some((old_context, old_text)) = self.order.pop_front() {
                let empty = self.contexts.get_mut(&old_context).is_some_and(|map| {
                    map.remove(old_text.as_ref());
                    map.is_empty()
                });
                if empty {
                    self.contexts.remove(&old_context);
                }
            }
        }
        let text: Rc<str> = Rc::from(text);
        self.contexts
            .entry(context)
            .or_default()
            .insert(text.clone(), value);
        self.order.push_back((context, text));
    }
    pub(crate) fn clear(&mut self) {
        self.contexts.clear();
        self.order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context(pixels: i32, script: &'static str) -> FontContext {
        FontContext {
            pixels,
            bold: false,
            face: "CozetteVector",
            script,
        }
    }
    #[test]
    fn long_render_does_not_add_another_idle_frame_interval() {
        let start = Instant::now();
        let mut pacer = FramePacer::new(start);
        assert!(pacer.due(start));
        pacer.begin_frame(start);
        pacer.request();
        assert!(pacer.due(start + Duration::from_millis(35)));
        pacer.begin_frame(start + Duration::from_millis(35));
        assert!(!pacer.dirty());
        assert!(!pacer.due(start + Duration::from_millis(100)));
    }
    #[test]
    fn requests_coalesce_without_postponing_the_due_frame() {
        let start = Instant::now();
        let mut pacer = FramePacer::new(start);
        pacer.begin_frame(start);
        for ms in 1..16 {
            pacer.request();
            assert!(!pacer.due(start + Duration::from_millis(ms)));
        }
        assert!(pacer.due(start + FRAME_INTERVAL));
        pacer.begin_frame(start + FRAME_INTERVAL);
        assert!(!pacer.dirty());
    }
    #[test]
    fn glyph_decisions_keep_text_size_and_regional_script_separate() {
        let mut choices = FontChoiceCache::default();
        choices.insert(context(16, "ru"), "Размер кисти", 1usize);
        choices.insert(context(16, "ja"), "日本語", 2);
        assert_eq!(choices.get(context(16, "ru"), "Размер кисти"), Some(1));
        assert_eq!(choices.get(context(17, "ru"), "Размер кисти"), None);
        assert_eq!(choices.get(context(16, "zh"), "日本語"), None);
        assert_eq!(choices.get(context(16, "ru"), "Размер кисти 2"), None);
        choices.clear();
        assert_eq!(choices.get(context(16, "ru"), "Размер кисти"), None);
    }
    #[test]
    fn dynamic_labels_cannot_grow_the_font_cache_without_bound() {
        let mut choices = FontChoiceCache::default();
        for i in 0..2048 {
            choices.insert(context(16, "ru"), &format!("{i} px"), i);
        }
        assert_eq!(choices.order.len(), 1024);
        assert_eq!(choices.get(context(16, "ru"), "0 px"), None);
        assert_eq!(choices.get(context(16, "ru"), "2047 px"), Some(2047));
        choices.insert(context(16, "ru"), "2047 px", 99);
        assert_eq!(choices.order.len(), 1024);
        assert_eq!(choices.get(context(16, "ru"), "2047 px"), Some(99));
        choices.insert(context(20, "ko"), "크기", 100);
        assert_eq!(choices.order.len(), 1024);
        assert_eq!(choices.get(context(20, "ko"), "크기"), Some(100));
        choices.clear();
        choices.insert(context(16, "ru"), &"x".repeat(4096), 0);
        assert_eq!(choices.order.len(), 0);
    }
}
