use serde_json::{json, Value};

pub(crate) type ProseSize = (&'static str, usize);
pub(crate) const SMALL_PROSE: ProseSize = ("small", 6);
pub(crate) const MEDIUM_PROSE: ProseSize = ("medium", 96);
pub(crate) const LARGE_PROSE: ProseSize = ("large", 384);
pub(crate) const VERY_LARGE_PROSE: ProseSize = ("very-large", 1536);
pub(crate) const PROSE_CORPUS_SIZES: [ProseSize; 4] =
    [SMALL_PROSE, MEDIUM_PROSE, LARGE_PROSE, VERY_LARGE_PROSE];
pub(crate) const PROSE_TITLE_PREFIX: &str = "Editor performance: ";
pub(crate) const PROSE_VARIANT_TITLE_PREFIX: &str = "EdiTor performance: ";
pub(crate) const PROSE_TITLE_LEVEL: u8 = 2;
const BLOCKQUOTE_PERIOD: usize = 16;
const PARAGRAPH_NUMBER_WIDTH: usize = 4;
const PROSE_SENTENCE: &str =
    "Native editing preserves each word, its formatting, and the caret while the document grows.";
const PROSE_TAIL: &str = " Unicode: café, e\u{301}, 東京, 😀. Escapes: <tag> & \"quote\".";
const PROSE_TAIL_HTML: &str =
    " Unicode: café, e\u{301}, 東京, 😀. Escapes: &lt;tag&gt; &amp; &#34;quote&#34;.";
const BOLD_TEXT: &str = "bold emphasis";
const ITALIC_TEXT: &str = "italic detail";
const LINK_TEXT: &str = "linked reference";
const MARK_SEPARATOR: &str = ", ";
const LINK_BASE: &str = "https://example.com/editor-performance/";
const PASTE_LINE_BREAK: char = '\n';

pub(crate) struct ProseCorpus {
    title: String,
    blocks: usize,
}

impl ProseCorpus {
    pub(crate) fn new(title_prefix: &str, (name, blocks): ProseSize) -> Self {
        Self {
            title: format!("{title_prefix}{name}"),
            blocks,
        }
    }

    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    fn quoted(index: usize) -> bool {
        index % BLOCKQUOTE_PERIOD == BLOCKQUOTE_PERIOD - 1
    }

    fn lead(index: usize) -> String {
        format!("Paragraph {index:0PARAGRAPH_NUMBER_WIDTH$}: {PROSE_SENTENCE} ")
    }

    pub(crate) fn html(&self) -> String {
        let mut html = format!(
            "<html><body><h{PROSE_TITLE_LEVEL}>{}</h{PROSE_TITLE_LEVEL}>",
            self.title
        );
        for index in 0..self.blocks {
            let paragraph = format!(
                "<p>{}<b>{BOLD_TEXT}</b>{MARK_SEPARATOR}<i>{ITALIC_TEXT}</i>{MARK_SEPARATOR}<a href=\"{LINK_BASE}{index}\">{LINK_TEXT}</a>.{PROSE_TAIL_HTML}</p>",
                Self::lead(index)
            );
            if Self::quoted(index) {
                html.push_str(&format!("<blockquote>{paragraph}</blockquote>"));
            } else {
                html.push_str(&paragraph);
            }
        }
        html.push_str("</body></html>");
        html
    }

    pub(crate) fn document_json(&self) -> Value {
        let mut content = vec![json!({
            "type": "heading",
            "attrs": { "level": PROSE_TITLE_LEVEL },
            "content": [{ "type": "text", "text": self.title }]
        })];
        for index in 0..self.blocks {
            let paragraph = json!({ "type": "paragraph", "content": [
                { "type": "text", "text": Self::lead(index) },
                { "type": "text", "text": BOLD_TEXT, "marks": [{ "type": "bold" }] },
                { "type": "text", "text": MARK_SEPARATOR },
                { "type": "text", "text": ITALIC_TEXT, "marks": [{ "type": "italic" }] },
                { "type": "text", "text": MARK_SEPARATOR },
                {
                    "type": "text",
                    "text": LINK_TEXT,
                    "marks": [{ "type": "link", "attrs": { "href": format!("{LINK_BASE}{index}") } }]
                },
                { "type": "text", "text": format!(".{PROSE_TAIL}") }
            ] });
            content.push(if Self::quoted(index) {
                json!({ "type": "blockquote", "content": [paragraph] })
            } else {
                paragraph
            });
        }
        json!({ "type": "doc", "content": content })
    }
}

pub(crate) fn prose_paste_text(utf16_len: usize) -> String {
    let line = format!("{PROSE_SENTENCE}{PROSE_TAIL}");
    let mut text = String::new();
    let mut length = 0usize;
    for character in std::iter::repeat(line.chars().chain([PASTE_LINE_BREAK])).flatten() {
        let width = character.len_utf16();
        if length + width > utf16_len {
            break;
        }
        text.push(character);
        length += width;
    }
    text
}
