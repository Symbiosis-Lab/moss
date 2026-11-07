//! Adapt the last build's ArticleMap to the portable reference index input.
use crate::build::scan::article_map::ArticleMap;
use moss_core::resolve::editor::urls::{UrlMap, Article};

pub fn reference_map(map: &ArticleMap) -> UrlMap {
    UrlMap {
        articles: map.articles.iter().map(|(url, a)| (url.clone(), Article {
            source_path: a.source_path.clone(), frontmatter: a.frontmatter.clone(),
        })).collect(),
        pages: map.pages.clone(), page_titles: map.page_titles.clone(),
        dir_overrides: map.dir_overrides.clone(), generated: map.generated.clone(),
        moved: map.terms.iter().filter_map(|(key, term)| term.claimed_by.as_ref().map(|claim| (key.clone(), claim.clone()))).collect(),
        folder_indexes: map.folder_indexes.iter().map(|(dir, url)| (dir.clone(), url.clone())).collect(),
    }
}

#[cfg(test)]
mod tests {
    use crate::build::terms::TermSite;

    /// Only a claimed term's URL moves, to the claiming page's key.
    #[test]
    fn a_claimed_term_moves_and_an_unclaimed_one_does_not() {
        let mut map = super::ArticleMap::new();
        let term = |claimed_by: Option<&str>| TermSite { display: String::new(), claimed_by: claimed_by.map(str::to_string), parent: None };
        map.terms.insert("authors/ma".into(), term(Some("about/ma/")));
        map.terms.insert("tags/home".into(), term(Some("")));
        map.terms.insert("authors/scarly".into(), term(None));
        let moved = super::reference_map(&map).moved;
        assert_eq!(moved.len(), 2);
        assert_eq!(moved["authors/ma"], "about/ma/");
        assert_eq!(moved["tags/home"], "");
    }
}
