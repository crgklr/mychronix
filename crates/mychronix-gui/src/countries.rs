//! Country names for the ISO 3166 codes used in AiM's track database.

pub fn name(code: &str) -> Option<&'static str> {
    Some(match code {
        "AE" => "United Arab Emirates",
        "AM" => "Armenia",
        "AO" => "Angola",
        "AR" => "Argentina",
        "AT" => "Austria",
        "AU" => "Australia",
        "AZ" => "Azerbaijan",
        "BA" => "Bosnia and Herzegovina",
        "BB" => "Barbados",
        "BE" => "Belgium",
        "BG" => "Bulgaria",
        "BH" => "Bahrain",
        "BM" => "Bermuda",
        "BN" => "Brunei",
        "BO" => "Bolivia",
        "BR" => "Brazil",
        "BW" => "Botswana",
        "BY" => "Belarus",
        "CA" => "Canada",
        "CH" => "Switzerland",
        "CL" => "Chile",
        "CN" => "China",
        "CO" => "Colombia",
        "CR" => "Costa Rica",
        "CY" => "Cyprus",
        "CZ" => "Czechia",
        "DE" => "Germany",
        "DK" => "Denmark",
        "DO" => "Dominican Republic",
        "DZ" => "Algeria",
        "EC" => "Ecuador",
        "EE" => "Estonia",
        "EG" => "Egypt",
        "ES" => "Spain",
        "ET" => "Ethiopia",
        "FI" => "Finland",
        "FR" => "France",
        "GB" => "United Kingdom",
        "GE" => "Georgia",
        "GR" => "Greece",
        "GT" => "Guatemala",
        "GY" => "Guyana",
        "HR" => "Croatia",
        "HT" => "Haiti",
        "HU" => "Hungary",
        "ID" => "Indonesia",
        "IE" => "Ireland",
        "IL" => "Israel",
        "IM" => "Isle of Man",
        "IN" => "India",
        "IR" => "Iran",
        "IT" => "Italy",
        "JM" => "Jamaica",
        "JO" => "Jordan",
        "JP" => "Japan",
        "KE" => "Kenya",
        "KH" => "Cambodia",
        "KR" => "South Korea",
        "KW" => "Kuwait",
        "KZ" => "Kazakhstan",
        "LA" => "Laos",
        "LB" => "Lebanon",
        "LK" => "Sri Lanka",
        "LT" => "Lithuania",
        "LU" => "Luxembourg",
        "LV" => "Latvia",
        "MA" => "Morocco",
        "MC" => "Monaco",
        "MD" => "Moldova",
        "ME" => "Montenegro",
        "MG" => "Madagascar",
        "MK" => "North Macedonia",
        "MO" => "Macao",
        "MT" => "Malta",
        "MX" => "Mexico",
        "MY" => "Malaysia",
        "NA" => "Namibia",
        "NC" => "New Caledonia",
        "NG" => "Nigeria",
        "NL" => "Netherlands",
        "NO" => "Norway",
        "NZ" => "New Zealand",
        "OM" => "Oman",
        "PA" => "Panama",
        "PE" => "Peru",
        "PH" => "Philippines",
        "PK" => "Pakistan",
        "PL" => "Poland",
        "PR" => "Puerto Rico",
        "PT" => "Portugal",
        "PY" => "Paraguay",
        "QA" => "Qatar",
        "RE" => "Réunion",
        "RO" => "Romania",
        "RS" => "Serbia",
        "RU" => "Russia",
        "SA" => "Saudi Arabia",
        "SE" => "Sweden",
        "SG" => "Singapore",
        "SI" => "Slovenia",
        "SK" => "Slovakia",
        "SN" => "Senegal",
        "SV" => "El Salvador",
        "TH" => "Thailand",
        "TN" => "Tunisia",
        "TR" => "Türkiye",
        "TT" => "Trinidad and Tobago",
        "TW" => "Taiwan",
        "UA" => "Ukraine",
        "US" => "United States",
        "UY" => "Uruguay",
        "VE" => "Venezuela",
        "VN" => "Vietnam",
        "ZA" => "South Africa",
        "ZW" => "Zimbabwe",
        _ => return None,
    })
}

/// The country from the system locale (`LC_ALL`, `LC_ADDRESS`, then `LANG`),
/// e.g. `en_US.UTF-8` → `US`.
pub fn system_country() -> Option<String> {
    ["LC_ALL", "LC_ADDRESS", "LANG"]
        .iter()
        .find_map(|var| locale_region(&std::env::var(var).ok()?))
        // Apps opened from the Dock on a Mac get no locale variables; ask the system.
        .or_else(|| mac_default("AppleLocale").and_then(|l| mac_region(&l)))
}

/// "en_US.UTF-8" → "US".
fn locale_region(value: &str) -> Option<String> {
    let region = value.split(['.', '@']).next()?.split('_').nth(1)?;
    (region.len() == 2 && region.chars().all(|c| c.is_ascii_alphabetic())).then(|| region.to_ascii_uppercase())
}

/// A Mac's "en_US", or "en_US@rg=gbzzzz" when the region was set apart from
/// the language ("GB").
fn mac_region(locale: &str) -> Option<String> {
    if let Some(rg) = locale.split("rg=").nth(1) {
        let r = rg.get(..2)?;
        return r.chars().all(|c| c.is_ascii_alphabetic()).then(|| r.to_ascii_uppercase());
    }
    locale_region(locale)
}

/// A global Mac setting (`defaults read -g <key>`); None elsewhere.
pub fn mac_default(key: &str) -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let out = std::process::Command::new("defaults").args(["read", "-g", key]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod region_tests {
    use super::*;

    #[test]
    fn regions_from_locales() {
        assert_eq!(locale_region("en_US.UTF-8").as_deref(), Some("US"));
        assert_eq!(locale_region("C"), None);
        assert_eq!(mac_region("en_US").as_deref(), Some("US"));
        assert_eq!(mac_region("en_US@rg=gbzzzz").as_deref(), Some("GB"));
    }
}
