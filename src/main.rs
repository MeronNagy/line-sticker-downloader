use regex::Regex;
use scraper::{Html, Selector};

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
            eprintln!("{} is not a url skipping.", url);
        }
    }

    Ok(())
}

async fn download_stickers(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let resp = client.get(url).send().await?.text().await?;
    let document = Html::parse_document(resp.as_str());

    let directory = sanitize_directory_name(&get_title(&document));
    println!("Creating dir: {}", directory);
    std::fs::create_dir_all(&directory)?;

    let selector = Selector::parse(r#"span.mdCMN09Image"#).unwrap();

    let mut img_urls = std::collections::HashSet::new();
    for element in document.select(&selector) {
        if let Some(style) = element.value().attr("style") {
            img_urls.insert(extract_image_url(style));
        }
    }

    for img_url in img_urls {
        let re = Regex::new(r"/(\d+)/").unwrap();
        let id = re.captures(&img_url).unwrap().get(1).unwrap().as_str();
        let file_path = format!("{}/{:03}.png", directory, id);

        if std::path::Path::new(&file_path).exists() {
            println!("File already downloaded: {}", file_path);
            continue;
        }

        println!("Downloading: {}", img_url);
        let img_resp = client.get(&img_url).send().await?;
        let img_bytes = img_resp.bytes().await?;
        std::fs::write(&file_path, img_bytes)?;
    }

    Ok(())
}

fn get_title(document: &Html) -> String {
    let selector = Selector::parse(r#"p[data-test="sticker-name-title"]"#).unwrap();

    let element = document.select(&selector).next().unwrap();
    let text = element.text().next().unwrap();

    text.to_string()
}

fn extract_image_url(style: &str) -> String {
    println!("Extracting image: {}", style);
    let url_start = style.find("https").unwrap();

    style[url_start..].split("?").next().unwrap().to_string()
}

fn sanitize_directory_name(name: &str) -> String {
    let name = name.replace("/", "_");

    let re = Regex::new(r#"[<>:"\\|?*]"#).unwrap();
    re.replace_all(&name, "").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_title() {
        let document = Html::parse_document(r#"<div class="mdCMN38Item0lHead"><p class="mdCMN38Item01Ttl" data-test="sticker-name-title">We are NewJeans☆</p></div>"#);
        let result = get_title(&document);
        assert_eq!(result, "We are NewJeans☆")
    }

    #[test]
    fn test_extract_image_url() {
        let result = extract_image_url("background-image:url(https://stickershop.line-scdn.net/stickershop/v1/sticker/714004505/android/sticker.png?v=1);");
        assert_eq!(result, "https://stickershop.line-scdn.net/stickershop/v1/sticker/714004505/android/sticker.png")
    }

    #[test]
    fn test_sanitize_directory_name() {
        // Should replace '/' with '_'
        let result = sanitize_directory_name("Ranma1/2");
        assert_eq!(result, "Ranma1_2");

        // Should replace ':' with empty string
        let result = sanitize_directory_name("The Legend of Zelda: Breath of the Wild");
        assert_eq!(result, "The Legend of Zelda Breath of the Wild");

        // Should not replace anything.
        let result = sanitize_directory_name("We are NewJeans☆");
        assert_eq!(result, "We are NewJeans☆");
        let result = sanitize_directory_name("Pikachu, Switch Out! Come Back!");
        assert_eq!(result, "Pikachu, Switch Out! Come Back!");
        let result = sanitize_directory_name("Yarn Yoshi & Poochy Stickers");
        assert_eq!(result, "Yarn Yoshi & Poochy Stickers");
    }
}