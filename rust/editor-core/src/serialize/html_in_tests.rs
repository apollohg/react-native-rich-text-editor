use scraper::Html;

use super::{parse_html_fragment, presized_parse_node_capacity, MIN_HTML_BYTES_PER_PRESIZED_NODE};
use crate::test_support::prose_corpus::{ProseCorpus, PROSE_CORPUS_SIZES, PROSE_TITLE_PREFIX};

const TAG_HEAVY_REPEATS: usize = 100_000;

fn corpus_html() -> Vec<(String, String)> {
    PROSE_CORPUS_SIZES
        .into_iter()
        .map(|size| {
            let (name, _) = size;
            (
                name.to_owned(),
                ProseCorpus::new(PROSE_TITLE_PREFIX, size).html(),
            )
        })
        .collect()
}

fn edge_case_html() -> Vec<(String, String)> {
    [
        ("empty", String::new()),
        ("plain text", "no markup at all".to_owned()),
        (
            "comment",
            "<p>a</p><!-- <b><i>not tags</i></b> --><p>b</p>".to_owned(),
        ),
        (
            "attribute",
            "<p title=\"a<b<c\">x</p><a href='?q=<i>'>y</a>".to_owned(),
        ),
        (
            "raw text",
            "<script>if (a<b) { c<d }</script><style>a<b{}</style><textarea><p>t</p></textarea>"
                .to_owned(),
        ),
        ("malformed", "<<<p>a<</p>< b><//>".to_owned()),
        (
            "implied table",
            "<table><tr><td>a</td></tr></table>".to_owned(),
        ),
        (
            "escaped code",
            "<pre><code>&lt;div&gt;&lt;/div&gt; a < b</code></pre>".to_owned(),
        ),
    ]
    .into_iter()
    .map(|(label, html)| (label.to_owned(), html))
    .collect()
}

#[test]
fn presized_parse_tree_matches_the_default_scraper_parse() {
    for (label, html) in corpus_html().into_iter().chain(edge_case_html()) {
        let presized = parse_html_fragment(&html);
        let reference = Html::parse_fragment(&html);
        assert_eq!(
            presized.html(),
            reference.html(),
            "{label}: serialized tree"
        );
        assert_eq!(
            presized.tree.nodes().count(),
            reference.tree.nodes().count(),
            "{label}: node count"
        );
    }
}

#[test]
fn presized_capacity_covers_real_documents_and_bounds_tag_heavy_input() {
    for (label, html) in corpus_html() {
        let nodes = Html::parse_fragment(&html).tree.nodes().count();
        let capacity = presized_parse_node_capacity(&html);
        assert!(
            nodes <= capacity,
            "{label}: {nodes} parsed nodes must fit the {capacity}-node reservation"
        );
    }
    let tag_heavy = format!("<!--{}-->", "<a".repeat(TAG_HEAVY_REPEATS));
    let capacity = presized_parse_node_capacity(&tag_heavy);
    assert_eq!(
        capacity,
        tag_heavy.len() / MIN_HTML_BYTES_PER_PRESIZED_NODE + 1,
        "tag-like text inside a comment is capped by input length"
    );
    assert!(
        Html::parse_fragment(&tag_heavy).tree.nodes().count() < capacity,
        "the comment parses to far fewer nodes than the bounded reservation"
    );
}
