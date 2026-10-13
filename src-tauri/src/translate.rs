use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct Translation {
    pub text: String,
    pub source: String,
    pub target: String,
}

#[derive(Deserialize)]
struct Response {
    translations: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    detected_source_language: String,
    text: String,
}

/// Kullanıcının seçtiği çeviri yönü.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    /// Türkçe → İngilizce, diğer her dil → Türkçe.
    Auto,
    /// Metni Türkçe kabul et, İngilizceye çevir.
    TrEn,
    /// Kaynağı DeepL algılasın, Türkçeye çevir.
    ToTr,
}

impl Mode {
    pub fn parse(s: Option<&str>) -> Self {
        match s {
            Some("tr_en") => Mode::TrEn,
            Some("to_tr") => Mode::ToTr,
            _ => Mode::Auto,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Guess {
    /// Kesin Türkçe: DeepL'e algılatmadan TR kaynak olarak gönderilir.
    Turkish,
    /// Muhtemelen Türkçe ama başka dillerde de geçen kelimeler var.
    Maybe,
    Other,
}

/// Yalnızca Türkçede bulunan harfler (ç, ö, ü Almanca/Fransızcada da var).
const TR_LETTERS: &[char] = &['ğ', 'Ğ', 'ı', 'İ', 'ş', 'Ş'];

/// Başka dillerde neredeyse hiç geçmeyen Türkçe kelimeler (günlük ve argo dahil,
/// Türkçe karaktersiz yazımlarıyla birlikte).
const TR_STRONG_WORDS: &[&str] = &[
    "naber", "nbr", "napıyon", "napiyon", "napıyorsun", "napiyorsun", "selam", "slm", "merhaba",
    "mrb", "tamam", "tmm", "günaydın", "gunaydin", "iyiyim", "nasılsın", "nasilsin", "nasıl",
    "nasil", "teşekkürler", "tesekkurler", "teşekkür", "tesekkur", "sağol", "sagol", "eyvallah",
    "kanka", "abi", "hadi", "haydi", "değil", "degil", "için", "icin", "çok", "cok", "evet",
    "hayır", "hayir", "peki", "yani", "şimdi", "simdi", "bugün", "bugun", "yarın", "yarin",
    "neden", "niye", "nerede", "nerde", "kadar", "şey", "sey", "lütfen", "lutfen", "iyi", "güzel",
    "guzel", "misin", "mısın", "musun", "müsün", "miyim", "mıyım", "geliyorum", "gidiyorum",
];

/// Türkçe olabilecek ama başka dillerde de geçen kısa kelimeler
/// (ör. "ama" İspanyolca, "var" İsveççe, "ve" Çekçe).
const TR_WEAK_WORDS: &[&str] = &[
    "ve", "bir", "bu", "ben", "sen", "biz", "siz", "var", "yok", "ile", "gibi", "ama", "daha",
    "ne", "mi", "da", "de",
];

/// Türkçeye özgü fiil/çoğul ekleri (Türkçe karaktersiz yazımlar dahil).
const TR_SUFFIXES: &[&str] = &[
    "yor", "yorum", "yorsun", "yoruz", "yorsunuz", "yorlar", "iyo", "ıyo", "uyo", "üyo", "yosun",
    "yoz", "yom", "ları", "leri", "larda", "lerde", "lardan", "lerden", "ımız", "imiz", "sınız",
    "siniz",
];

/// Kısa metinlerde DeepL'in Türkçe yerine sıkça algıladığı diller.
const TR_LOOKALIKES: &[&str] = &[
    "KMR", "KU", "AZ", "UZ", "TK", "TT", "KK", "KY", "BA", "CRH", "GAG",
];

fn guess(text: &str) -> Guess {
    if text.chars().any(|c| TR_LETTERS.contains(&c)) {
        return Guess::Turkish;
    }
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .collect();

    // Ek kontrolü kısa kelimelerde yanıltır ("mayor", "suyo"), o yüzden en az 6 harf.
    let strong = words.iter().any(|w| {
        TR_STRONG_WORDS.contains(w)
            || (w.chars().count() >= 6 && TR_SUFFIXES.iter().any(|s| w.ends_with(s)))
    });
    if strong {
        Guess::Turkish
    } else if words.iter().any(|w| TR_WEAK_WORDS.contains(w)) {
        Guess::Maybe
    } else {
        Guess::Other
    }
}

fn is_turkish_like(lang: &str) -> bool {
    lang == "TR" || TR_LOOKALIKES.contains(&lang)
}

fn finish(item: Item, source: &str, target: &str) -> Translation {
    Translation {
        text: item.text,
        source: source.to_string(),
        target: target.split('-').next().unwrap_or(target).to_string(),
    }
}

pub async fn translate(
    client: &reqwest::Client,
    key: &str,
    text: &str,
    mode: Mode,
) -> Result<Translation, String> {
    let tr_to_en = || async {
        let item = request(client, key, text, Some("TR"), "EN-US").await?;
        Ok::<_, String>(finish(item, "TR", "EN-US"))
    };

    match mode {
        Mode::TrEn => return tr_to_en().await,
        Mode::ToTr => {
            let item = request(client, key, text, None, "TR").await?;
            let src = item.detected_source_language.to_uppercase();
            return Ok(finish(item, &src, "TR"));
        }
        Mode::Auto => {}
    }

    let target = match guess(text) {
        Guess::Turkish => return tr_to_en().await,
        Guess::Maybe => "EN-US",
        Guess::Other => "TR",
    };

    let item = request(client, key, text, None, target).await?;
    let src = item.detected_source_language.to_uppercase();

    if src == "TR" && target == "EN-US" {
        Ok(finish(item, "TR", target))
    } else if is_turkish_like(&src) {
        // Türkçe ya da Türkçeye benzeyen bir dil algılandı: Türkçe kabul edip yeniden çevir.
        tr_to_en().await
    } else if target == "EN-US" {
        // Türkçe sandık ama değilmiş.
        let item = request(client, key, text, None, "TR").await?;
        Ok(finish(item, &src, "TR"))
    } else {
        Ok(finish(item, &src, target))
    }
}

async fn request(
    client: &reqwest::Client,
    key: &str,
    text: &str,
    source: Option<&str>,
    target: &str,
) -> Result<Item, String> {
    // Ücretsiz plan anahtarları ":fx" ile biter ve ayrı bir uç noktaya gider.
    let host = if key.ends_with(":fx") {
        "https://api-free.deepl.com"
    } else {
        "https://api.deepl.com"
    };

    let mut body = serde_json::json!({ "text": [text], "target_lang": target });
    if let Some(src) = source {
        body["source_lang"] = src.into();
    }

    let res = client
        .post(format!("{host}/v2/translate"))
        .header("Authorization", format!("DeepL-Auth-Key {key}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "Bağlantı zaman aşımına uğradı.".to_string()
            } else {
                "DeepL'e bağlanılamadı. İnternet bağlantını kontrol et.".to_string()
            }
        })?;

    match res.status().as_u16() {
        200 => {}
        403 => return Err("DeepL API anahtarı geçersiz.".into()),
        456 => return Err("Bu ayki DeepL karakter kotası doldu.".into()),
        429 => return Err("Çok fazla istek gönderildi, biraz bekle.".into()),
        s if s >= 500 => return Err("DeepL şu an yanıt vermiyor, sonra tekrar dene.".into()),
        s => return Err(format!("DeepL hatası ({s}).")),
    }

    let body: Response = res
        .json()
        .await
        .map_err(|_| "DeepL yanıtı okunamadı.".to_string())?;
    body.translations
        .into_iter()
        .next()
        .ok_or_else(|| "DeepL boş yanıt döndürdü.".to_string())
}

#[cfg(test)]
mod tests {
    use super::{guess, Guess, Mode};

    #[test]
    fn confident_turkish() {
        for t in [
            "naber",
            "slm nbr",
            "nasilsin kanka",
            "merhaba dünya",
            "Işık",
            "geliyorum",
            "napiyosun",
            "yarin gelir misin",
            "evlerde",
        ] {
            assert_eq!(guess(t), Guess::Turkish, "{t}");
        }
    }

    #[test]
    fn ambiguous_turkish() {
        assert_eq!(guess("bu ne"), Guess::Maybe);
        assert_eq!(guess("ama"), Guess::Maybe);
    }

    #[test]
    fn not_turkish() {
        for t in [
            "hello",
            "good morning",
            "Guten Morgen, schön",
            "the mayor said",
            "a killer smile",
            "embryo",
        ] {
            assert_eq!(guess(t), Guess::Other, "{t}");
        }
    }

    #[test]
    fn parses_mode() {
        assert_eq!(Mode::parse(Some("tr_en")), Mode::TrEn);
        assert_eq!(Mode::parse(Some("to_tr")), Mode::ToTr);
        assert_eq!(Mode::parse(None), Mode::Auto);
        assert_eq!(Mode::parse(Some("x")), Mode::Auto);
    }
}
