// Dark mode toggle functionality
(function () {
  "use strict";

  const themeToggle = document.getElementById("theme-toggle");
  const themeIcon = document.getElementById("theme-icon");
  const html = document.documentElement;

  // Get saved theme or default to 'light'
  function getTheme() {
    const saved = localStorage.getItem("theme");
    if (saved) {
      return saved;
    }
    // Check system preference
    if (
      window.matchMedia &&
      window.matchMedia("(prefers-color-scheme: dark)").matches
    ) {
      return "dark";
    }
    return "light";
  }

  // Apply theme
  function setTheme(theme) {
    if (theme === "dark") {
      html.setAttribute("data-bs-theme", "dark");
      if (themeIcon) {
        themeIcon.classList.remove("fa-moon");
        themeIcon.classList.add("fa-sun");
      }
    } else {
      html.setAttribute("data-bs-theme", "light");
      if (themeIcon) {
        themeIcon.classList.remove("fa-sun");
        themeIcon.classList.add("fa-moon");
      }
    }
    localStorage.setItem("theme", theme);
  }

  // Toggle theme
  function toggleTheme() {
    const currentTheme = html.getAttribute("data-bs-theme") || "light";
    const newTheme = currentTheme === "dark" ? "light" : "dark";
    setTheme(newTheme);
  }

  // Initialize theme immediately and also on DOM ready
  function initTheme() {
    const theme = getTheme();
    setTheme(theme);
  }

  // Run immediately if DOM is ready, otherwise wait
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", initTheme);
  } else {
    initTheme();
  }

  // Also run after a short delay to ensure everything is loaded
  setTimeout(initTheme, 100);

  // Listen for system theme changes
  if (window.matchMedia) {
    window
      .matchMedia("(prefers-color-scheme: dark)")
      .addEventListener("change", (e) => {
        if (!localStorage.getItem("theme")) {
          setTheme(e.matches ? "dark" : "light");
        }
      });
  }

  // Attach toggle button
  if (themeToggle) {
    themeToggle.addEventListener("click", toggleTheme);
  }
})();
