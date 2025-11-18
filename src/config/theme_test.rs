//! Test utility for theme loading
//! This can be used to test theme loading without running the full TUI

#[cfg(test)]
mod tests {
    use super::super::theme_loader::ThemeLoader;
    use std::path::PathBuf;

    #[test]
    fn test_load_rose_theme() {
        // Test loading the rose theme if it exists
        let theme_path = PathBuf::from(std::env::var("HOME").unwrap())
            .join(".config")
            .join("flux9s")
            .join("skins")
            .join("rose.yaml");
        
        if theme_path.exists() {
            match ThemeLoader::load_theme("rose") {
                Ok(theme) => {
                    println!("Successfully loaded rose theme");
                    println!("Header context color: {:?}", theme.header_context);
                    println!("Text primary color: {:?}", theme.text_primary);
                    println!("Status ready color: {:?}", theme.status_ready);
                }
                Err(e) => {
                    eprintln!("Failed to load rose theme: {}", e);
                }
            }
        } else {
            println!("Rose theme file not found at: {:?}", theme_path);
        }
    }
}


