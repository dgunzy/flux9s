#!/bin/bash
# Fetch metadata about flux9s from various sources
# This script is run during the GitHub Pages build process

set -e

METADATA_FILE="${1:-metadata.json}"
REPO="dgunzy/flux9s"
CRATE="flux9s"

echo "Fetching metadata for ${REPO}..."

# Initialize metadata object
cat > "${METADATA_FILE}" <<EOF
{
  "crates_downloads": 0,
  "homebrew_downloads": 0,
  "github_stars": 0,
  "github_releases": 0,
  "last_updated": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF

# Fetch crates.io download count
echo "Fetching crates.io download count..."
# Crates.io API doesn't require auth for public data, but we can add User-Agent header
CRATES_RESPONSE=$(curl -s -H "User-Agent: flux9s-docs/1.0" "https://crates.io/api/v1/crates/${CRATE}" 2>/dev/null || echo "{}")
CRATES_DOWNLOADS=$(echo "${CRATES_RESPONSE}" | jq -r '.crate.downloads // 0' 2>/dev/null || echo "0")

# Verify we got valid data
if [ "${CRATES_DOWNLOADS}" = "null" ] || [ -z "${CRATES_DOWNLOADS}" ]; then
  CRATES_DOWNLOADS="0"
fi

if [ "${CRATES_DOWNLOADS}" != "0" ]; then
  jq ".crates_downloads = ${CRATES_DOWNLOADS}" "${METADATA_FILE}" > "${METADATA_FILE}.tmp" && mv "${METADATA_FILE}.tmp" "${METADATA_FILE}"
  echo "  Crates.io downloads: ${CRATES_DOWNLOADS}"
else
  echo "  Crates.io downloads: 0"
fi

# Fetch GitHub stars and releases (requires GITHUB_TOKEN if rate limited)
GITHUB_TOKEN="${GITHUB_TOKEN:-}"
if [ -z "${GITHUB_TOKEN}" ]; then
  echo "  Warning: GITHUB_TOKEN not set, GitHub API calls may be rate limited"
fi

# Fetch GitHub repository info
echo "Fetching GitHub repository info..."
GITHUB_HEADERS=()
if [ -n "${GITHUB_TOKEN}" ]; then
  GITHUB_HEADERS=(-H "Authorization: token ${GITHUB_TOKEN}")
fi

GITHUB_RESPONSE=$(curl -s "${GITHUB_HEADERS[@]}" "https://api.github.com/repos/${REPO}" || echo "{}")
GITHUB_STARS=$(echo "${GITHUB_RESPONSE}" | jq -r '.stargazers_count // 0' 2>/dev/null || echo "0")
if [ -n "${GITHUB_STARS}" ] && [ "${GITHUB_STARS}" != "null" ]; then
  jq ".github_stars = ${GITHUB_STARS}" "${METADATA_FILE}" > "${METADATA_FILE}.tmp" && mv "${METADATA_FILE}.tmp" "${METADATA_FILE}"
  echo "  GitHub stars: ${GITHUB_STARS}"
else
  echo "  Failed to fetch GitHub stars"
fi

# Fetch GitHub releases count and Homebrew downloads (sum of all release asset downloads)
echo "Fetching GitHub releases and Homebrew download counts..."
# Fetch releases with pagination - get first 100 releases (should be enough)
RELEASES_RESPONSE=$(curl -s "${GITHUB_HEADERS[@]}" "https://api.github.com/repos/${REPO}/releases?per_page=100" || echo "[]")
GITHUB_RELEASES=$(echo "${RELEASES_RESPONSE}" | jq '. | length' 2>/dev/null || echo "0")

# Calculate total Homebrew downloads by summing all release asset download counts
# Homebrew typically downloads tar.gz/zip files from releases
HOMEBREW_DOWNLOADS=$(echo "${RELEASES_RESPONSE}" | jq '[.[]?.assets[]?.download_count // 0] | add // 0' 2>/dev/null || echo "0")

# If we got 100 releases, there might be more, but we'll just show 100+
if [ "${GITHUB_RELEASES}" = "100" ]; then
  GITHUB_RELEASES="100+"
fi

if [ -n "${GITHUB_RELEASES}" ] && [ "${GITHUB_RELEASES}" != "null" ] && [ "${GITHUB_RELEASES}" != "0" ]; then
  # For numeric values, update JSON; for "100+", we'll handle it differently
  if [[ "${GITHUB_RELEASES}" =~ ^[0-9]+$ ]]; then
    jq ".github_releases = ${GITHUB_RELEASES}" "${METADATA_FILE}" > "${METADATA_FILE}.tmp" && mv "${METADATA_FILE}.tmp" "${METADATA_FILE}"
  else
    # For "100+", store as string in a different field or just use 100
    jq ".github_releases = 100" "${METADATA_FILE}" > "${METADATA_FILE}.tmp" && mv "${METADATA_FILE}.tmp" "${METADATA_FILE}"
  fi
  echo "  GitHub releases: ${GITHUB_RELEASES}"
else
  echo "  Failed to fetch GitHub releases"
fi

# Update Homebrew downloads
if [ -n "${HOMEBREW_DOWNLOADS}" ] && [ "${HOMEBREW_DOWNLOADS}" != "null" ]; then
  jq ".homebrew_downloads = ${HOMEBREW_DOWNLOADS}" "${METADATA_FILE}" > "${METADATA_FILE}.tmp" && mv "${METADATA_FILE}.tmp" "${METADATA_FILE}"
  echo "  Homebrew downloads (from GitHub releases): ${HOMEBREW_DOWNLOADS}"
else
  echo "  Homebrew downloads: 0"
fi

echo "Metadata saved to ${METADATA_FILE}"
cat "${METADATA_FILE}"
