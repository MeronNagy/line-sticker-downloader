use regex::Regex;
use scraper::{Html, Selector};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, VecDeque};

#[derive(Debug, serde::Deserialize)]
struct Item {
    #[serde(rename = "productUrl")]
    product_url: String,
}

#[derive(Debug, serde::Deserialize)]
struct SearchResponse {
    #[serde(rename = "totalCount")]
    total_count: u32,
    items: Vec<Item>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <url1> <url2> ...", args[0]);
        std::process::exit(1);
    }

    for arg in &args[1..] {
        if url::Url::parse(arg).is_ok() {
            download_stickers(arg).await.unwrap_or_else(|err| {
                eprintln!("Failed to fetch stickers: {}", err);
            })
        } else {
            download_stickers_from_search_query("https://store.line.me", arg)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("Failed to fetch stickers: {}", err);
                })
        }
    }

    Ok(())
}

async fn download_stickers_from_search_query(
    base_url: &str,
    search_query: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let limit = 36;
    let mut offset = 0;

    loop {
        let url = format!(
            "{}/api/search/sticker?category=sticker&type=ALL&offset={}&limit={}&includeFacets=false&query={}",
            base_url,
            offset,
            limit,
            search_query,
        );

        let response = reqwest::get(&url).await?;
        let json: SearchResponse = response.json().await?;
        let total_count = json.total_count;
        let items = json.items;

        println!(
            "Fetching Stickers for query: {} Page: {}",
            search_query, offset
        );
        download_items(base_url, items).await?;

        offset += limit;
        if offset >= total_count {
            return Ok(());
        }
    }
}

async fn download_items(
    base_url: &str,
    items: Vec<Item>,
) -> Result<(), Box<dyn std::error::Error>> {
    for item in items {
        let url = format!("{}{}", base_url, item.product_url);
        download_stickers(&url).await?
    }

    Ok(())
}

async fn download_stickers(initial_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut url_queue: VecDeque<String> = VecDeque::new();
    url_queue.push_back(initial_url.to_string());

    while let Some(url) = url_queue.pop_back() {
        let response = reqwest::get(&url).await?.text().await?;
        let document = Html::parse_document(&response);

        if url.contains("/stickershop/author/") {
            println!("Fetching Stickers from: {}", url);

            url_queue.extend(extract_author_page_urls(url, document)?);
            continue;
        }

        let title = extract_title_from_document(&document)?;
        println!("Downloading {}", title);

        let directory = sanitize_directory_name(&title);
        for (id, value) in extract_sticker_data_from_document(&document)? {
            if let Some(url) = value.get("soundUrl").and_then(|v| v.as_str()) {
                if !url.is_empty() {
                    download_file(url, &id, &directory).await?;
                }
            }

            if let Some(url) = value.get("animationUrl").and_then(|v| v.as_str()) {
                if !url.is_empty() {
                    download_file(url, &id, &directory).await?;
                } else if let Some(url) = value.get("staticUrl").and_then(|v| v.as_str()) {
                    download_file(url, &id, &directory).await?;
                }
            }
        }
    }

    Ok(())
}

fn extract_author_page_urls(
    url: String,
    document: Html,
) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    let mut urls: BTreeSet<String> = BTreeSet::new();

    let author_item_selector = Selector::parse(r#"li[data-test="author-item"]"#)?;
    let a_selector = Selector::parse("a")?;
    for li in document.select(&author_item_selector) {
        if let Some(a_tag) = li.select(&a_selector).next() {
            if let Some(href) = a_tag.value().attr("href") {
                urls.insert(update_url(&url, href)?);
            }
        }
    }

    if let Some(href) = extract_next_button_href(document)? {
        urls.insert(update_url(&url, &href)?);
    }

    Ok(urls)
}

fn extract_next_button_href(document: Html) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let next_btn_selector = Selector::parse(r#"a[data-test="next-btn"]"#)?;
    if let Some(next_btn) = document.select(&next_btn_selector).next() {
        if let Some(next_href) = next_btn.value().attr("href") {
            return Ok(Some(next_href.to_string()));
        }
    }

    Ok(None)
}

fn update_url(url: &str, path_or_query_params: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut new_url = url::Url::parse(url)?;
    new_url.set_query(None);

    if path_or_query_params.starts_with('/') {
        if let Some((path, query)) = path_or_query_params.split_once('?') {
            new_url.set_path(path);
            new_url.set_query(Some(query));
        } else {
            new_url.set_path(path_or_query_params);
        }
    } else {
        new_url.set_query(Some(path_or_query_params.trim_start_matches('?')));
    }

    Ok(new_url.to_string())
}

fn sanitize_directory_name(name: &str) -> String {
    let name = name.replace("/", "_");

    let invalid_chars_regex = Regex::new(r#"[<>:"\\|?*]"#).unwrap();
    invalid_chars_regex.replace_all(&name, "").to_string()
}

fn extract_title_from_document(document: &Html) -> Result<String, String> {
    let selector = Selector::parse(r#"p[data-test="sticker-name-title"]"#).unwrap();

    if let Some(element) = document.select(&selector).next() {
        let text = element.text().next().unwrap_or_default();
        Ok(text.to_string())
    } else {
        Err("Could not find the sticker-name-title in the document. Please check that the URL points to a valid sticker page.".to_string())
    }
}

fn extract_sticker_data_from_document(document: &Html) -> Result<HashMap<String, Value>, String> {
    let selector = Selector::parse("li.FnStickerPreviewItem").unwrap();

    let mut sticker_data_map: HashMap<String, Value> = HashMap::new();
    for element in document.select(&selector) {
        if let Some(data_preview) = element.value().attr("data-preview") {
            match serde_json::from_str::<Value>(data_preview) {
                Ok(json) => {
                    if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
                        sticker_data_map.insert(id.to_string(), json);
                    }
                }
                Err(err) => {
                    Err(format!("Could not parse sticker data: {}", err))?;
                }
            }
        }
    }

    Ok(sticker_data_map)
}

async fn download_file(
    url: &str,
    file_name: &str,
    directory: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !std::path::Path::new(directory).exists() {
        std::fs::create_dir_all(directory)?;
    }

    if let Some(extension) = extract_file_extension(url) {
        let file_path = format!("{}/{}.{}", directory, file_name, extension);
        let response = reqwest::get(url).await?;
        let bytes = response.bytes().await?;
        std::fs::write(&file_path, bytes)?;
        Ok(())
    } else {
        Err("Could not extract file extension from url".into())
    }
}

fn extract_file_extension(url: &str) -> Option<String> {
    let parsed_url = url::Url::parse(url).ok()?;
    let path = parsed_url.path();
    path.rsplit('/')
        .next()
        .and_then(|filename| filename.rsplit('.').next().filter(|ext| *ext != filename))
        .map(|ext| ext.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_directory_name() {
        // Should replace '/' with '_'
        let actual = sanitize_directory_name("Ranma1/2");
        assert_eq!(actual, "Ranma1_2");

        // Should replace ':' with empty string
        let actual = sanitize_directory_name("The Legend of Zelda: Breath of the Wild");
        assert_eq!(actual, "The Legend of Zelda Breath of the Wild");

        // Should not replace anything.
        let actual = sanitize_directory_name("We are NewJeans☆");
        assert_eq!(actual, "We are NewJeans☆");
        let actual = sanitize_directory_name("Pikachu, Switch Out! Come Back!");
        assert_eq!(actual, "Pikachu, Switch Out! Come Back!");
        let actual = sanitize_directory_name("Yarn Yoshi & Poochy Stickers");
        assert_eq!(actual, "Yarn Yoshi & Poochy Stickers");
    }

    #[test]
    fn test_extract_title_from_document() {
        let document = Html::parse_document(
            r#"
            <div class="mdCMN38Item0lHead"><p class="mdCMN38Item01Ttl" data-test="sticker-name-title">We are NewJeans☆</p></div>
        "#,
        );
        let actual = extract_title_from_document(&document);
        assert_eq!(actual.unwrap(), "We are NewJeans☆");
    }

    #[test]
    fn test_extract_title_from_document_returns_error() {
        let document = Html::parse_document(r#"<div></div>"#);
        let actual = extract_title_from_document(&document);

        assert!(actual.is_err());
    }

    #[test]
    fn test_extract_sticker_data_from_document() {
        let document = Html::parse_document(
            r#"<ul>
                    <li class="for_testing"></li>
                    <li class="mdCMN09Li FnStickerPreviewItem animation_sound-sticker " data-preview="{ &quot;type&quot; : &quot;animation_sound&quot;, &quot;id&quot; : &quot;20578528&quot;, &quot;staticUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578528/iPhone/sticker@2x.png?v=1&quot;, &quot;fallbackStaticUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578528/iPhone/sticker@2x.png?v=1&quot;, &quot;animationUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578528/iPhone/sticker_animation@2x.png?v=1&quot;, &quot;popupUrl&quot; : &quot;&quot;, &quot;soundUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578528/android/sticker_sound.m4a?v=1&quot; }" data-test="sticker-item"></li>
                    <li class="for_testing" data-preview="{ &quot;type&quot; : &quot;animation&quot;, &quot;id&quot; : &quot;1&quot;}"></li>
                    <li class="mdCMN09Li FnStickerPreviewItem animation-sticker " data-preview="{ &quot;type&quot; : &quot;animation&quot;, &quot;id&quot; : &quot;651763951&quot;, &quot;staticUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/651763951/iPhone/sticker@2x.png?v=2&quot;, &quot;fallbackStaticUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/651763951/iPhone/sticker@2x.png?v=2&quot;, &quot;animationUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/651763951/iPhone/sticker_animation@2x.png?v=2&quot;, &quot;popupUrl&quot; : &quot;&quot;, &quot;soundUrl&quot; : &quot;&quot; }" data-test="sticker-item">
                    <li class="for_testing"></li>
                </ul>"#,
        );

        let sticker_data = extract_sticker_data_from_document(&document).unwrap();

        assert_eq!(sticker_data.len(), 2);

        assert!(sticker_data.contains_key("20578528"));
        let sticker_20578528 = sticker_data.get("20578528").unwrap();
        assert_eq!(
            sticker_20578528["animationUrl"].as_str().unwrap(),
            "https://stickershop.line-scdn.net/stickershop/v1/sticker/20578528/iPhone/sticker_animation@2x.png?v=1"
        );

        assert!(sticker_data.contains_key("651763951"));
        let sticker_651763951 = sticker_data.get("651763951").unwrap();
        assert_eq!(
            sticker_651763951["staticUrl"].as_str().unwrap(),
            "https://stickershop.line-scdn.net/stickershop/v1/sticker/651763951/iPhone/sticker@2x.png?v=2"
        );
    }

    #[test]
    fn test_extract_sticker_data_from_document_error() {
        let document = Html::parse_document(
            r#"
            <ul>
                <li class="FnStickerPreviewItem" data-preview="{ []{{]]}{{fsdfsf435 }">
            </ul>
        "#,
        );

        let actual = extract_sticker_data_from_document(&document);

        assert!(actual.is_err());
    }

    #[test]
    fn test_extract_file_extension() {
        let actual = extract_file_extension("https://stickershop.line-scdn.net/stickershop/v1/sticker/20578528/android/sticker_sound.m4a?v=1").unwrap();
        assert_eq!(actual, "m4a");

        let actual = extract_file_extension("https://stickershop.line-scdn.net/stickershop/v1/sticker/20578528/iPhone/sticker_animation@2x.png?v=1").unwrap();
        assert_eq!(actual, "png");
    }

    #[test]
    fn test_extract_file_extension_invalid() {
        let actual = extract_file_extension("https://stickershop.line-scdn.net/stickershop/v1/sticker/20578528/android/sticker_sound");
        assert!(actual.is_none());
    }

    #[tokio::test]
    async fn test_download_file_error_no_extension() {
        let actual = download_file(
            "https://stickershop.line-scdn.net/stickershop/v1/sticker/20578528/android/sticker_sound",
            "test",
            "test",
        ).await;
        assert!(actual.is_err());
    }
    #[tokio::test]
    async fn test_download_stickers_animated_with_sound() {
        let mut server = mockito::Server::new_async().await;

        let url = server.url();
        let _m = server.mock("GET", "/test")
            .with_status(200)
            .with_header("content-type", "text/html;charset=UTF-8")
            .with_body(r#"
                <p class="mdCMN38Item01Ttl" data-test="sticker-name-title">Pokémon Pixel Art: Gold & Silver Edition</p>
                <ul>
                    <li class="mdCMN09Li FnStickerPreviewItem animation_sound-sticker " data-preview="{ &quot;type&quot; : &quot;animation_sound&quot;, &quot;id&quot; : &quot;20578551&quot;, &quot;staticUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578551/iPhone/sticker@2x.png?v=1&quot;, &quot;fallbackStaticUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578551/iPhone/sticker@2x.png?v=1&quot;, &quot;animationUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578551/iPhone/sticker_animation@2x.png?v=1&quot;, &quot;popupUrl&quot; : &quot;&quot;, &quot;soundUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578551/android/sticker_sound.m4a?v=1&quot; }" data-test="sticker-item"></li>
                </ul>
            "#)
            .create_async()
            .await;

        download_stickers(format!("{}/test", url).as_str())
            .await
            .unwrap();
        let dir_path = std::path::Path::new("Pokémon Pixel Art Gold & Silver Edition");
        assert!(
            dir_path.exists(),
            "Directory 'Pokémon Pixel Art Gold & Silver Edition' does not exist"
        );
        let file_path = dir_path.join("20578551.m4a");
        assert!(file_path.exists(), "File '20578551.m4a' does not exist");
        let file_path = dir_path.join("20578551.png");
        assert!(file_path.exists(), "File '20578551.png' does not exist");

        delete_directory_if_exists("Pokémon Pixel Art Gold & Silver Edition");
    }

    #[tokio::test]
    async fn test_download_stickers_static() {
        let mut server = mockito::Server::new_async().await;

        let url = server.url();
        let _m = server.mock("GET", "/test")
            .with_status(200)
            .with_header("content-type", "text/html;charset=UTF-8")
            .with_body(r#"
                <p class="mdCMN38Item01Ttl" data-test="sticker-name-title">THE POWERPUFF GIRLS X NEWJEANS</p>
                <ul>
                    <li class="mdCMN09Li FnStickerPreviewItem static-sticker " data-preview="{ &quot;type&quot; : &quot;static&quot;, &quot;id&quot; : &quot;616659318&quot;, &quot;staticUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/616659318/android/sticker.png?v=2&quot;, &quot;fallbackStaticUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/616659318/android/sticker.png?v=2&quot;, &quot;animationUrl&quot; : &quot;&quot;, &quot;popupUrl&quot; : &quot;&quot;, &quot;soundUrl&quot; : &quot;&quot; }" data-test="sticker-item"></li>
                </ul>
            "#)
            .create_async()
            .await;

        download_stickers(&format!("{}/test", url)).await.unwrap();
        let dir_path = std::path::Path::new("THE POWERPUFF GIRLS X NEWJEANS");
        assert!(
            dir_path.exists(),
            "Directory 'THE POWERPUFF GIRLS X NEWJEANS' does not exist"
        );
        let file_path = dir_path.join("616659318.png");
        assert!(file_path.exists(), "File '616659318.png' does not exist");

        delete_directory_if_exists("THE POWERPUFF GIRLS X NEWJEANS");
    }

    #[tokio::test]
    async fn test_download_stickers_no_stickers_on_page() {
        let mut server = mockito::Server::new_async().await;

        let url = server.url();
        let _m = server.mock("GET", "/test")
            .with_status(200)
            .with_header("content-type", "text/html;charset=UTF-8")
            .with_body(r#"
                <p class="mdCMN38Item01Ttl" data-test="sticker-name-title">THE POWERPUFF GIRLS X NEWJEANS</p>
            "#)
            .create_async()
            .await;

        let actual = download_stickers(&format!("{}/test", url)).await;
        assert!(actual.is_ok(), "{}", actual.unwrap_err());
    }

    #[tokio::test]
    async fn test_download_stickers_author_page_invalid_href() {
        let mut server = mockito::Server::new_async().await;

        let url = server.url();
        let _m = server
            .mock("GET", "/stickershop/author/test")
            .with_status(200)
            .with_header("content-type", "text/html;charset=UTF-8")
            .with_body(
                r#"
                <ul>
                    <li class="mdCMN02Li" data-test="author-item">
                        <a href="/test">
                    </li>
                </ul>
            "#,
            )
            .create_async()
            .await;

        let _m2 = server
            .mock("GET", "/test")
            .with_status(200)
            .with_header("content-type", "text/html;charset=UTF-8")
            .with_body(
                r#"
                <div></div>
            "#,
            )
            .create_async()
            .await;

        let actual = download_stickers(&format!("{}/stickershop/author/test", url)).await;
        assert!(actual.is_err(), "{}", actual.unwrap_err());
    }

    fn delete_directory_if_exists(directory: &str) {
        let directory_path = std::path::Path::new(directory);
        if directory_path.exists() {
            std::fs::remove_dir_all(directory_path).expect("Failed to remove directory");
        }
    }

    #[test]
    fn test_extract_author_page_urls() {
        let document = Html::parse_document(
            r#"
            <ul>
                <li class="mdCMN02Li" data-test="author-item">
                    <a href="/stickershop/product/32279/en">
                </li>
            </ul>
            <a class="mdCMN14Next" href="?page=2" data-test="next-btn">Next</a>
        "#,
        );

        let actual = extract_author_page_urls(
            "https://store.line.me/stickershop/author/32/en".to_string(),
            document,
        )
        .unwrap();

        assert_eq!(actual.len(), 2);
        assert!(actual.contains("https://store.line.me/stickershop/product/32279/en"));
        assert!(actual.contains("https://store.line.me/stickershop/author/32/en?page=2"));
    }

    #[test]
    fn test_update_url() {
        let actual = update_url(
            "https://store.line.me/stickershop/author/32/en",
            "/stickershop/product/20095/en",
        )
        .unwrap();
        assert_eq!(actual, "https://store.line.me/stickershop/product/20095/en");

        let actual =
            update_url("https://store.line.me/stickershop/author/32/en", "?page=2").unwrap();
        assert_eq!(
            actual,
            "https://store.line.me/stickershop/author/32/en?page=2"
        );

        let actual =
            update_url("https://store.line.me/stickershop/author/32/en", "page=2").unwrap();
        assert_eq!(
            actual,
            "https://store.line.me/stickershop/author/32/en?page=2"
        );

        let actual = update_url("https://store.line.me/stickershop/author/32/en", "abc").unwrap();
        assert_eq!(actual, "https://store.line.me/stickershop/author/32/en?abc");
    }

    #[test]
    fn test_update_url_error() {
        let actual = update_url("not-a-url", "/stickershop/product/20095/en");
        assert!(actual.is_err(), "{}", actual.unwrap_err());
    }

    #[tokio::test]
    async fn test_download_items() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let _m = server
            .mock("GET", "/test")
            .with_status(200)
            .with_header("content-type", "text/html;charset=UTF-8")
            .with_body(r#"
                <p class="mdCMN38Item01Ttl" data-test="sticker-name-title">Test Download Items</p>
                <ul>
                    <li class="mdCMN09Li FnStickerPreviewItem animation_sound-sticker " data-preview="{ &quot;type&quot; : &quot;animation_sound&quot;, &quot;id&quot; : &quot;20578551&quot;, &quot;staticUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578551/iPhone/sticker@2x.png?v=1&quot;, &quot;fallbackStaticUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578551/iPhone/sticker@2x.png?v=1&quot;, &quot;animationUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578551/iPhone/sticker_animation@2x.png?v=1&quot;, &quot;popupUrl&quot; : &quot;&quot;, &quot;soundUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578551/android/sticker_sound.m4a?v=1&quot; }" data-test="sticker-item"></li>
                </ul>
            "#)
            .create_async()
            .await;

        let items = vec![Item {
            product_url: String::from("/test"),
        }];

        let actual = download_items(&url, items).await;
        assert!(actual.is_ok(), "{}", actual.unwrap_err());

        let dir_path = std::path::Path::new("Test Download Items");
        assert!(
            dir_path.exists(),
            "Directory 'Test Download Items' does not exist"
        );
        let file_path = dir_path.join("20578551.m4a");
        assert!(file_path.exists(), "File '20578551.m4a' does not exist");
        let file_path = dir_path.join("20578551.png");
        assert!(file_path.exists(), "File '20578551.png' does not exist");

        delete_directory_if_exists("Test Download Items");
    }

    #[tokio::test]
    async fn test_download_stickers_from_search_query() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let _m = server
            .mock("GET", "/api/search/sticker?category=sticker&type=ALL&offset=0&limit=36&includeFacets=false&query=hatsune+miku")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"totalCount":1,"items":[{"productUrl": "/test"}]}"#)
            .create_async()
            .await;

        let _m2 = server
            .mock("GET", "/test")
            .with_status(200)
            .with_header("content-type", "text/html;charset=UTF-8")
            .with_body(
                r#"
                <p class="mdCMN38Item01Ttl" data-test="sticker-name-title">Hatsune Miku</p>
            "#,
            )
            .create_async()
            .await;

        let actual = download_stickers_from_search_query(&url, "hatsune+miku").await;
        assert!(actual.is_ok(), "{}", actual.unwrap_err());

        let dir_path = std::path::Path::new("Hatsune Miku");

        assert!(
            !dir_path.exists(),
            "Directory 'Hatsune Miku' should not exist because no download happened."
        );

        delete_directory_if_exists("Hatsune Miku");
    }
}
