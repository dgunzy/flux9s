// Fetch and display project metadata
(async function () {
  try {
    // Fetch metadata from our API endpoint (generated during build)
    const basePath =
      window.location.pathname.split("/").slice(0, -1).join("/") || "";
    const response = await fetch(`${basePath}/metadata.json`);
    if (!response.ok) {
      throw new Error("Failed to fetch metadata");
    }

    const metadata = await response.json();

    // Update crates.io downloads
    const cratesEl = document.getElementById("crates-downloads");
    if (cratesEl && metadata.crates_downloads) {
      cratesEl.textContent = formatNumber(metadata.crates_downloads);
    }

    // Update GitHub stars
    const starsEl = document.getElementById("github-stars");
    if (starsEl && metadata.github_stars) {
      starsEl.textContent = formatNumber(metadata.github_stars);
    }

    // Update GitHub releases
    const releasesEl = document.getElementById("github-releases");
    if (releasesEl && metadata.github_releases) {
      releasesEl.textContent = formatNumber(metadata.github_releases);
    }
  } catch (error) {
    console.error("Error loading metadata:", error);
    // Set default values on error
    document.getElementById("crates-downloads").textContent = "-";
    document.getElementById("github-stars").textContent = "-";
    document.getElementById("github-releases").textContent = "-";
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
