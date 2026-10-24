//! What a question to Mimir names in everyday words: the entity kinds it means
//! ("aircraft" is an `air-track`) and the places it is about ("over London").
//!
//! Both are plain built-in tables. A question is reduced to lower-case words with
//! simple plurals folded ("ferries" is "ferry", "ships" is "ship") and every phrase is
//! matched on whole words, so "Londonderry" never matches "London".

/// Everyday words for entity kinds: a phrase (singular, lower case, words separated
/// by single spaces) and the kinds it means. A hit scores like a word found in the
/// entity's kind.
pub const KIND_SYNONYMS: &[(&str, &[&str])] = &[
    ("aircraft", &["air-track"]),
    ("airplane", &["air-track"]),
    ("aeroplane", &["air-track"]),
    ("plane", &["air-track"]),
    ("flight", &["air-track"]),
    ("jet", &["air-track"]),
    ("helicopter", &["air-track"]),
    ("balloon", &["air-balloon"]),
    ("ship", &["sea-vessel", "sea-track"]),
    ("vessel", &["sea-vessel", "sea-track"]),
    ("boat", &["sea-vessel", "sea-track"]),
    ("tanker", &["sea-vessel", "sea-track"]),
    ("ferry", &["sea-vessel", "sea-track"]),
    ("train", &["land-train"]),
    ("rail", &["land-train"]),
    ("railway", &["land-train"]),
    ("satellite", &["space-satellite"]),
    ("power cut", &["utility-power-outage"]),
    ("blackout", &["utility-power-outage"]),
    ("outage", &["utility-power-outage"]),
    ("flood", &["event-flood"]),
    ("flooding", &["event-flood"]),
    ("earthquake", &["event-earthquake"]),
    ("quake", &["event-earthquake"]),
    ("fire", &["event-fire"]),
    ("wildfire", &["event-fire"]),
    ("roadwork", &["land-road-disruption"]),
    ("road", &["land-road-disruption"]),
    ("camera", &["land-camera"]),
    ("windfarm", &["sea-windfarm"]),
    ("wind farm", &["sea-windfarm"]),
];

/// A named place: its display name, the phrases that name it (lower case, whole
/// words), and a circle `(lat, lon, radius_km)`.
pub struct Place {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub lat: f64,
    pub lon: f64,
    pub radius_km: f64,
}

const fn place(
    name: &'static str,
    aliases: &'static [&'static str],
    lat: f64,
    lon: f64,
    radius_km: f64,
) -> Place {
    Place {
        name,
        aliases,
        lat,
        lon,
        radius_km,
    }
}

/// The places presenters ask about. Approximate centres; radii cover the city or
/// the water body roughly, not an administrative boundary.
pub const PLACES: &[Place] = &[
    place("London", &["london"], 51.5074, -0.1278, 35.0),
    place("Manchester", &["manchester"], 53.4808, -2.2426, 20.0),
    place("Birmingham", &["birmingham"], 52.4862, -1.8904, 20.0),
    place("Glasgow", &["glasgow"], 55.8642, -4.2518, 20.0),
    place("Edinburgh", &["edinburgh"], 55.9533, -3.1883, 20.0),
    place("Belfast", &["belfast"], 54.5973, -5.9301, 20.0),
    place("Derry", &["londonderry", "derry"], 54.9966, -7.3086, 15.0),
    place("Dublin", &["dublin"], 53.3498, -6.2603, 25.0),
    place("Cardiff", &["cardiff"], 51.4816, -3.1791, 20.0),
    place("Bristol", &["bristol"], 51.4545, -2.5879, 20.0),
    place("Leeds", &["leeds"], 53.8008, -1.5491, 20.0),
    place("Liverpool", &["liverpool"], 53.4084, -2.9916, 20.0),
    place(
        "Newcastle",
        &["newcastle", "newcastle upon tyne"],
        54.9783,
        -1.6178,
        20.0,
    ),
    place("Southampton", &["southampton"], 50.9097, -1.4044, 20.0),
    place("Aberdeen", &["aberdeen"], 57.1497, -2.0943, 20.0),
    place(
        "Heathrow",
        &["heathrow", "london heathrow"],
        51.4700,
        -0.4543,
        10.0,
    ),
    place(
        "Gatwick",
        &["gatwick", "london gatwick"],
        51.1537,
        -0.1821,
        10.0,
    ),
    place(
        "Stansted",
        &["stansted", "london stansted"],
        51.8860,
        0.2389,
        10.0,
    ),
    place(
        "the English Channel",
        &["english channel"],
        50.2,
        -1.5,
        150.0,
    ),
    place(
        "the Dover Strait",
        &[
            "dover strait",
            "strait of dover",
            "straits of dover",
            "dover",
        ],
        51.0,
        1.45,
        35.0,
    ),
    place("the North Sea", &["north sea"], 56.0, 3.0, 400.0),
    place("the Irish Sea", &["irish sea"], 53.7, -4.8, 120.0),
    place("Dubai", &["dubai"], 25.2048, 55.2708, 30.0),
    place("Abu Dhabi", &["abu dhabi"], 24.4539, 54.3773, 30.0),
    place("Doha", &["doha", "qatar"], 25.2854, 51.5310, 25.0),
    place("Bahrain", &["bahrain", "manama"], 26.0667, 50.5577, 40.0),
    place("Kuwait", &["kuwait", "kuwait city"], 29.3759, 47.9774, 50.0),
    place("Muscat", &["muscat"], 23.5880, 58.3829, 30.0),
    place(
        "the Strait of Hormuz",
        &["strait of hormuz", "straits of hormuz", "hormuz"],
        26.5667,
        56.25,
        60.0,
    ),
    place("Riyadh", &["riyadh"], 24.7136, 46.6753, 35.0),
    place("Tehran", &["tehran"], 35.6892, 51.3890, 35.0),
];

/// Fold a simple English plural: "ferries" is "ferry", "ships" is "ship"; "gas",
/// "class" and short words are left alone.
fn singular(w: &str) -> String {
    if w.len() > 4 && w.ends_with("ies") {
        format!("{}y", &w[..w.len() - 3])
    } else if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") && !w.ends_with("us") {
        w[..w.len() - 1].to_string()
    } else {
        w.to_string()
    }
}

/// The question as `" w1 w2 .. "`: lower case, singular, one space between and
/// around, so a phrase matches only on whole words.
fn padded_words(question: &str, fold_plurals: bool) -> String {
    let words: Vec<String> = question
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| {
            if fold_plurals {
                singular(w)
            } else {
                w.to_string()
            }
        })
        .collect();
    format!(" {} ", words.join(" "))
}

/// The entity kinds the question names in everyday words, one entry per phrase hit
/// (a phrase naming two kinds gives both).
pub fn synonym_kinds(question: &str) -> Vec<&'static str> {
    let q = padded_words(question, true);
    let mut kinds = Vec::new();
    for (phrase, ks) in KIND_SYNONYMS {
        if q.contains(&format!(" {phrase} ")) {
            for k in *ks {
                if !kinds.contains(k) {
                    kinds.push(*k);
                }
            }
        }
    }
    kinds
}

/// The places the question names, in table order. Longer phrases are matched first
/// and consume their words, so "London Heathrow" is Heathrow alone.
pub fn places_named(question: &str) -> Vec<&'static Place> {
    let mut q = padded_words(question, false);
    let mut aliases: Vec<(&str, usize)> = PLACES
        .iter()
        .enumerate()
        .flat_map(|(i, p)| p.aliases.iter().map(move |a| (*a, i)))
        .collect();
    aliases.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    let mut hit = vec![false; PLACES.len()];
    for (alias, i) in aliases {
        let needle = format!(" {alias} ");
        while let Some(at) = q.find(&needle) {
            hit[i] = true;
            // Blank the phrase but keep its bounding spaces.
            q.replace_range(at + 1..at + needle.len() - 1, &"#".repeat(alias.len()));
        }
    }
    PLACES
        .iter()
        .zip(hit)
        .filter_map(|(p, h)| h.then_some(p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(q: &str) -> Vec<&'static str> {
        places_named(q).iter().map(|p| p.name).collect()
    }

    #[test]
    fn everyday_words_name_kinds() {
        assert_eq!(
            synonym_kinds("How many aircraft are over London?"),
            ["air-track"]
        );
        assert_eq!(synonym_kinds("Any planes or jets?"), ["air-track"]);
        assert_eq!(
            synonym_kinds("Ferries and tankers in the Channel"),
            ["sea-vessel", "sea-track"]
        );
        assert_eq!(
            synonym_kinds("Any power cuts or floods?"),
            ["utility-power-outage", "event-flood"]
        );
        assert_eq!(synonym_kinds("Wind farms offshore"), ["sea-windfarm"]);
        assert_eq!(synonym_kinds("trains delayed"), ["land-train"]);
        assert_eq!(synonym_kinds("hot air balloons"), ["air-balloon"]);
        assert!(synonym_kinds("What is happening?").is_empty());
        // Whole words only: "shipping" is not "ship", "firework" is not "fire".
        assert!(synonym_kinds("shipping fireworks").is_empty());
    }

    #[test]
    fn places_match_whole_words_case_insensitively() {
        assert_eq!(
            names("How many aircraft are over LONDON right now?"),
            ["London"]
        );
        assert_eq!(names("Anything near Londonderry?"), ["Derry"]);
        assert_eq!(names("Derry and Dublin"), ["Derry", "Dublin"]);
        assert_eq!(
            names("ships in the Strait of Hormuz"),
            ["the Strait of Hormuz"]
        );
        assert_eq!(names("Flights at London Heathrow"), ["Heathrow"]);
        assert_eq!(names("London and Heathrow"), ["London", "Heathrow"]);
        assert_eq!(names("Abu Dhabi, Dubai"), ["Dubai", "Abu Dhabi"]);
        assert!(names("What is open?").is_empty());
        assert!(names("londoners").is_empty());
    }

    #[test]
    fn every_place_is_a_sensible_circle() {
        for p in PLACES {
            assert!((-90.0..=90.0).contains(&p.lat), "{}", p.name);
            assert!((-180.0..=180.0).contains(&p.lon), "{}", p.name);
            assert!(p.radius_km > 0.0 && p.radius_km <= 500.0, "{}", p.name);
            assert!(!p.aliases.is_empty(), "{}", p.name);
            for a in p.aliases {
                assert_eq!(*a, a.to_lowercase(), "{}", p.name);
            }
        }
    }
}
