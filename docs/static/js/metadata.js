// Fetch and display project metadata
(async function () {
  // Helper to safely get element and set text
  function safeSetText(elementId, text) {
    try {
      const el = document.getElementById(elementId);
      if (el) {
        el.textContent = text;
      }
    } catch (e) {
      console.warn("Failed to update element:", elementId, e);
    }
  }
  
  // Helper to set default values
  function setDefaults() {
    safeSetText("crates-downloads", "-");
    safeSetText("github-stars", "-");
    safeSetText("github-downloads", "-");
    safeSetText("github-releases", "-");
  }
  
  try {
    // Fetch metadata from our API endpoint (generated during build)
    const basePath =
      window.location.pathname.split("/").slice(0, -1).join("/") || "";
    const metadataUrl = `${basePath}/metadata.json`;
    
    // Use fetch with timeout
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), 5000); // 5 second timeout
    
    const response = await fetch(metadataUrl, {
      signal: controller.signal,
      cache: 'no-cache'
    });
    
    clearTimeout(timeoutId);
    
    if (!response.ok) {
      throw new Error(`Failed to fetch metadata: ${response.status} ${response.statusText}`);
    }

    const metadata = await response.json();

    // Update crates.io downloads
    if (metadata.crates_downloads !== undefined) {
      safeSetText("crates-downloads", formatNumber(metadata.crates_downloads));
    }

    // Update GitHub stars
    if (metadata.github_stars !== undefined) {
      safeSetText("github-stars", formatNumber(metadata.github_stars));
    }

    // Update GitHub binary downloads
    if (metadata.github_binary_downloads !== undefined) {
      safeSetText("github-downloads", formatNumber(metadata.github_binary_downloads));
    }

    // Update GitHub releases
    if (metadata.github_releases !== undefined) {
      safeSetText("github-releases", formatNumber(metadata.github_releases));
    }
  } catch (error) {
    // Log error but don't break the page
    if (error.name !== 'AbortError') {
      console.warn("Error loading metadata (this is non-critical):", error.message);
    }
    // Set default values on error - these are already "-" by default in HTML
    setDefaults();
  }
})();

function formatNumber(num) {
  if (num >= 1000000) {
    return (num / 1000000).toFixed(1) + "M";
  } else if (num >= 1000) {
    return (num / 1000).toFixed(1) + "K";
  }
  return num.toString();
}
