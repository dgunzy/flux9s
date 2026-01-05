// Font loading detection and fallback handling
// Detects if Font Awesome fonts loaded successfully, shows text fallbacks if not

(function() {
  'use strict';
  
  // Check if Font Awesome fonts are available
  function checkFontLoaded(fontFamily) {
    if (!document.fonts || !document.fonts.check) {
      // Fallback for browsers without Font Loading API
      return false;
    }
    
    // Check if the font is loaded
    // Font Awesome 6 Free (solid) uses weight 900
    // Font Awesome 6 Brands uses weight 400
    if (fontFamily.includes('Free')) {
      return document.fonts.check('900 1em "' + fontFamily + '"');
    } else if (fontFamily.includes('Brands')) {
      return document.fonts.check('400 1em "' + fontFamily + '"');
    }
    return false;
  }
  
  // Wait for fonts to load, then check
  function detectFontLoading() {
    if (!document.fonts) {
      // No Font Loading API - assume fonts might not load, show fallbacks
      setTimeout(function() {
        document.documentElement.classList.add('fonts-failed');
      }, 2000);
      return;
    }
    
    // Wait for fonts to be ready
    document.fonts.ready.then(function() {
      // Check both Font Awesome font families
      const freeLoaded = checkFontLoaded('Font Awesome 6 Free');
      const brandsLoaded = checkFontLoaded('Font Awesome 6 Brands');
      
      // If either font failed to load, show fallbacks
      if (!freeLoaded || !brandsLoaded) {
        document.documentElement.classList.add('fonts-failed');
        console.warn('Font Awesome fonts failed to load, showing text fallbacks');
      }
    }).catch(function() {
      // If font loading fails, show fallbacks
      document.documentElement.classList.add('fonts-failed');
    });
    
    // Also set a timeout - if fonts don't load within 3 seconds, show fallbacks
    setTimeout(function() {
      const freeLoaded = checkFontLoaded('Font Awesome 6 Free');
      const brandsLoaded = checkFontLoaded('Font Awesome 6 Brands');
      
      if (!freeLoaded || !brandsLoaded) {
        document.documentElement.classList.add('fonts-failed');
      }
    }, 3000);
  }
  
  // Run detection when DOM is ready
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', detectFontLoading);
  } else {
    detectFontLoading();
  }
})();

