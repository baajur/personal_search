use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;

mod Indexer {

    use chrono::prelude::*;

    use select::document;
    use std::collections::HashSet;
    use std::path::Path;
    use std::time::Duration;
    use tantivy::collector::TopDocs;
    use tantivy::query::QueryParser;
    use tantivy::schema::*;
    use tantivy::{Index, ReloadPolicy};
    pub fn search_index() -> std::result::Result<tantivy::Index, tantivy::TantivyError> {
        let system_path = ".private_search";
        let index_path = Path::new(system_path);
        // create it..
        if !index_path.is_dir() {
            println!("not found");
        }

        let directory = tantivy::directory::MmapDirectory::open(index_path);

        let mut schema_builder = Schema::builder();
        schema_builder.add_text_field("title", TEXT | STORED);
        schema_builder.add_text_field("url", TEXT | STORED);
        schema_builder.add_text_field("content", TEXT);
        schema_builder.add_text_field("domain", TEXT | STORED);
        schema_builder.add_text_field("context", TEXT);
        //schema_builder.add_text_field("preview_image", STORED);
        //schema_builder.add_text_field("preview_hash", STORED);
        //schema_builder.add_bytes_field("preview_image");
        schema_builder.add_i64_field("bookmarked", STORED | INDEXED);
        schema_builder.add_i64_field("pinned", STORED | INDEXED);
        schema_builder.add_i64_field("accessed_count", STORED);
        schema_builder.add_facet_field("outlinks");
        schema_builder.add_facet_field("tags");
        schema_builder.add_facet_field("keywords");
        schema_builder.add_date_field("added_at", STORED);
        schema_builder.add_date_field("last_accessed_at", STORED | INDEXED);

        let schema = schema_builder.build();
        match directory {
            Ok(dir) => Index::open_or_create(dir, schema),
            Err(_) => {
                println!("dir not found");
                Err(tantivy::TantivyError::SystemError(format!(
                    "could not open index directory {}",
                    system_path
                )))
            }
        }
    }

    pub fn get_url(url: &String) -> Result<String, reqwest::Error> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        let res = client.get(url).send()?;
        let body = res.text()?;

        Ok(body)
    }

    pub fn searcher(index: &Index) -> tantivy::LeasedItem<tantivy::Searcher> {
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .expect("reader");

        reader.searcher()
    }

    pub fn index_url(url: String) {
        let index = search_index();
        match index {
            Ok(index) => {
                if let Some(_doc_address) = find_url(&url, &index) {
                    // update?

                    //let searcher = searcher(&index);
                    // let retrieved_doc = searcher.doc(doc_address).expect("doc");
                    //    println!("{}", index.schema().to_json(&retrieved_doc));
                } else {
                    match get_url(&url) {
                        Ok(body) => {
                            let document = document::Document::from(body.as_str());
                            let mut doc = tantivy::Document::default();
                            let title = match document.find(select::predicate::Name("title")).next()
                            {
                                Some(node) => node.text(),
                                _ => "".to_string(),
                            };

                            let body = match document.find(select::predicate::Name("body")).next() {
                                Some(node) => node.text(),
                                _ => "".to_string(),
                            };

                            doc.add_text(index.schema().get_field("title").expect("title"), &title);
                            doc.add_text(
                                index.schema().get_field("content").expect("content"),
                                &body.split_whitespace().collect::<Vec<_>>().join(" "),
                            );
                            doc.add_text(index.schema().get_field("url").expect("url"), &url);
                            let parsed = reqwest::Url::parse(&url).expect("url pase");

                            doc.add_text(
                                index.schema().get_field("domain").expect("domain"),
                                parsed.domain().unwrap_or(""),
                            );
                            let found_urls = document
                                .find(select::predicate::Name("a"))
                                .filter_map(|n| n.attr("href"))
                                .map(str::to_string)
                                .collect::<HashSet<String>>();
                            for url in found_urls {
                                doc.add_facet(
                                    index.schema().get_field("outlinks").expect("outlinks"),
                                    Facet::from(&format!("/#{}", url.replacen("/", "?", 10000))),
                                );
                            }

                            let keywords = document
                                .find(select::predicate::Name("meta"))
                                .filter(|node| node.attr("name").unwrap_or("") == "keywords")
                                .filter_map(|n| n.attr("content"))
                                .flat_map(|s| s.split(','))
                                .map(str::to_string)
                                .collect::<Vec<String>>();

                            for keyword in keywords {
                                doc.add_facet(
                                    index.schema().get_field("keywords").expect("keywords"),
                                    Facet::from(&format!("/{}", keyword)),
                                );
                            }

                            let local: DateTime<Utc> = Utc::now();
                            doc.add_date(
                                index.schema().get_field("added_at").expect("added_at"),
                                &local,
                            );

                            doc.add_date(
                                index.schema().get_field("added_at").expect("added_at"),
                                &local,
                            );
                            doc.add_i64(index.schema().get_field("pinned").expect("pinned"), 0);
                            doc.add_i64(
                                index
                                    .schema()
                                    .get_field("accessed_count")
                                    .expect("accessed_count"),
                                1,
                            );
                            doc.add_i64(
                                index.schema().get_field("bookmarked").expect("bookmarked"),
                                0,
                            );

                            let mut index_writer = index.writer(50_000_000).expect("writer");
                            index_writer.add_document(doc);
                            index_writer.commit().expect("commit");
                        }
                        _ => {}
                    }
                };
            }
            _ => {}
        }
    }

    pub fn find_url(url: &String, index: &Index) -> std::option::Option<tantivy::DocAddress> {
        let searcher = searcher(&index);
        let query_parser = QueryParser::for_index(
            &index,
            vec![index.schema().get_field("url").expect("url field")],
        );

        let query = query_parser
            .parse_query(&format!("\"{}\"", url))
            .expect("query parse for url match");
        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(1))
            .expect("search");
        match top_docs.iter().nth(0) {
            Some((_, doc_address)) => Some(*doc_address),
            _ => None,
        }
    }
}

fn main() -> tantivy::Result<()> {
    Indexer::index_url("https://docs.rs/chrono/0.4.15/chrono/".to_string());
    let index = Indexer::search_index();
    match index {
        Ok(index) => {
            let searcher = Indexer::searcher(&index);

            let query_parser = QueryParser::for_index(
                &index,
                vec![index.schema().get_field("content").expect("content field")],
            );
            let query = query_parser.parse_query("chrono")?;
            let top_docs = searcher.search(&query, &TopDocs::with_limit(1))?;
            for (_score, doc_address) in top_docs {
                let retrieved_doc = searcher.doc(doc_address)?;
                println!(
                    "{:?}",
                    retrieved_doc
                        .get_all(index.schema().get_field("outlinks").expect("f"))
                        .iter()
                        .map(|s| match s {
                            tantivy::schema::Value::Facet(f) => f.to_path_string(),
                            _ => {
                                "".to_string()
                            }
                        })
                        .collect::<Vec<String>>()
                        .join(" ")
                );
            }
        }
        Err(_) => println!("count not access index"),
    }

    Ok(())
}
