use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <url1> <url2> ...", args[0]);
        std::process::exit(1);
    }

    for url in &args[1..] {
        if url::Url::parse(url).is_ok() {
            println!("Fetching {}", url);
            download_stickers(url).await?;
        } else {
            eprintln!("{} is not a valid URL skipping.", url);
        }
    }

    Ok(())
}

async fn download_stickers(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    let response = client.get(url).send().await?.text().await?;

    let document = Html::parse_document(response.as_str());

    let directory = sanitize_directory_name(
        &extract_title_from_document(&document).unwrap_or_else(|err| {
            eprintln!("Error: {}", err);
            std::process::exit(1);
        }),
    );

    println!("Creating dir: {}", directory);
    std::fs::create_dir_all(&directory)?;

    let sticker_data = extract_sticker_data_from_document(&document);



    Ok(())
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

fn extract_image_url_from_styles_from_document(
    document: &Html,
) -> std::collections::HashSet<String> {
    let selector = Selector::parse(r#"span.mdCMN09Image"#).unwrap();
    let mut image_urls = std::collections::HashSet::new();
    for element in document.select(&selector) {
        if let Some(style) = element.value().attr("style") {
            image_urls.insert(extract_image_url_from_style(style));
        }
    }
    image_urls
}

fn extract_image_url_from_style(style: &str) -> String {
    let url_start = style.find("https").unwrap();

    style[url_start..].split("?").next().unwrap().to_string()
}

fn generate_image_file_path(directory: &str, image_url: &str) -> String {
    let digit_regex = Regex::new(r"/(\d+)/").unwrap();
    let id = digit_regex
        .captures(image_url)
        .unwrap()
        .get(1)
        .unwrap()
        .as_str();
    format!("{}/{}.png", directory, id)
}

fn extract_sticker_data_from_document(document: &Html) -> std::collections::HashMap<String, Value> {
    let selector = Selector::parse("li.FnStickerPreviewItem").unwrap();

    let mut sticker_data_map: std::collections::HashMap<String, Value> = std::collections::HashMap::new();

    for element in document.select(&selector) {
        if let Some(data_preview) = element.value().attr("data-preview") {

            match serde_json::from_str::<Value>(&data_preview) {
                Ok(json) => {
                    if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
                        sticker_data_map.insert(id.to_string(), json);
                    }
                }
                Err(err) => {
                    eprintln!("Failed to parse JSON: {}", err);
                }
            }
        }
    }

    sticker_data_map
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
            r#"<div class="mdCMN38Item0lHead"><p class="mdCMN38Item01Ttl" data-test="sticker-name-title">We are NewJeans☆</p></div>"#,
        );
        let actual = extract_title_from_document(&document);
        assert_eq!(actual.unwrap(), "We are NewJeans☆");
    }

    #[test]
    fn test_extract_title_from_document_returns_error() {
        let document = Html::parse_document(r#"<div></div>"#);
        let result = extract_title_from_document(&document);

        assert!(result.is_err());
    }

    #[test]
    fn test_extract_image_url_from_styles_from_document() {
        let document = Html::parse_document(
            r#"
            <span class="mdCMN09Image" style="background-image:url(https://stickershop.line-scdn.net/stickershop/v1/sticker/651763950/iPhone/sticker@2x.png?v=2);"></span>
            <span class="mdCMN09Image FnPreview" style="background-image:url(https://stickershop.line-scdn.net/stickershop/v1/sticker/651763950/iPhone/sticker@2x.png?v=2);"></span>
            <span class="mdCMN09Image" style="background-image:url(https://stickershop.line-scdn.net/stickershop/v1/sticker/651763951/iPhone/sticker@2x.png?v=2);"></span>
            <span class="mdCMN09Image FnPreview" style="background-image:url(https://stickershop.line-scdn.net/stickershop/v1/sticker/651763951/iPhone/sticker@2x.png?v=2);"></span>
            "#,
        );
        let actual = extract_image_url_from_styles_from_document(&document);

        let expected_urls = vec![
            "https://stickershop.line-scdn.net/stickershop/v1/sticker/651763950/iPhone/sticker@2x.png",
            "https://stickershop.line-scdn.net/stickershop/v1/sticker/651763951/iPhone/sticker@2x.png",
        ];

        for url in &expected_urls {
            assert!(
                actual.contains(*url),
                "Expected URL {} not found in actual",
                url
            );
        }

        assert_eq!(
            actual.len(),
            expected_urls.len(),
            "The number of URLs extracted does not match the expected count"
        );
    }

    #[test]
    fn test_extract_image_url_from_style() {
        let actual = extract_image_url_from_style(
            "background-image:url(https://stickershop.line-scdn.net/stickershop/v1/sticker/714004505/android/sticker.png?v=1);"
        );
        assert_eq!(actual, "https://stickershop.line-scdn.net/stickershop/v1/sticker/714004505/android/sticker.png")
    }

    #[test]
    fn test_generate_image_file_path() {
        let actual = generate_image_file_path(
            "test",
            "https://stickershop.line-scdn.net/stickershop/v1/sticker/714004505/android/sticker.png"
        );
        assert_eq!(actual, "test/714004505.png");

        let actual = generate_image_file_path(
            "another_test",
            "https://stickershop.line-scdn.net/stickershop/v1/sticker/1/android/sticker.png",
        );
        assert_eq!(actual, "another_test/1.png")
    }

    #[test]
    fn test_extract_sticker_data_from_document() {
        let document = Html::parse_document(r#"
        <ul>
            <li class="for_testing"></li>
            <li class="mdCMN09Li FnStickerPreviewItem animation_sound-sticker " data-preview="{ &quot;type&quot; : &quot;animation_sound&quot;, &quot;id&quot; : &quot;20578528&quot;, &quot;staticUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578528/iPhone/sticker@2x.png?v=1&quot;, &quot;fallbackStaticUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578528/iPhone/sticker@2x.png?v=1&quot;, &quot;animationUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578528/iPhone/sticker_animation@2x.png?v=1&quot;, &quot;popupUrl&quot; : &quot;&quot;, &quot;soundUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/20578528/android/sticker_sound.m4a?v=1&quot; }" data-test="sticker-item"></li>
            <li class="for_testing" data-preview="{ &quot;type&quot; : &quot;animation&quot;, &quot;id&quot; : &quot;1&quot;}"></li>
            <li class="mdCMN09Li FnStickerPreviewItem animation-sticker " data-preview="{ &quot;type&quot; : &quot;animation&quot;, &quot;id&quot; : &quot;651763951&quot;, &quot;staticUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/651763951/iPhone/sticker@2x.png?v=2&quot;, &quot;fallbackStaticUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/651763951/iPhone/sticker@2x.png?v=2&quot;, &quot;animationUrl&quot; : &quot;https://stickershop.line-scdn.net/stickershop/v1/sticker/651763951/iPhone/sticker_animation@2x.png?v=2&quot;, &quot;popupUrl&quot; : &quot;&quot;, &quot;soundUrl&quot; : &quot;&quot; }" data-test="sticker-item">
            <li class="for_testing"></li>
        </ul>
        "#);

        let sticker_data = extract_sticker_data_from_document(&document);

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
    fn test_wip() {

    }

}
