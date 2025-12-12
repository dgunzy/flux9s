# Theme Customization Guide

This document describes the custom purple theme and dark mode implementation for the flux9s documentation site.

## Color Scheme

### Primary Colors

- **Primary Purple**: `#7c3aed` - Main brand color
- **Secondary Purple**: `#a78bfa` - Accent color
- **Info Purple**: `#8b5cf6` - Information elements

### Dark Mode

- **Dark Background**: `#0f172a` - Very dark blue-black
- **Dark Surface**: `#1e293b` - Dark slate for cards/surfaces
- **Dark Text**: `#f1f5f9` - Light text for readability

### Light Mode

- **Light Background**: `#ffffff` - White background
- **Light Surface**: `#f8fafc` - Very light gray
- **Light Text**: `#1e293b` - Dark text

## Custom Files

### SCSS Files

- `assets/scss/_variables_project.scss` - Primary color definitions
- `assets/scss/_variables_project_after_bs.scss` - Dark mode overrides
- `assets/scss/_styles_project.scss` - Custom component styles

### JavaScript Files

- `static/js/dark-mode.js` - Dark mode toggle functionality
- `static/js/metadata.js` - Project statistics display

### Layout Overrides

- `layouts/partials/navbar.html` - Custom navbar with dark mode toggle
- `layouts/partials/head-custom.html` - Custom head includes

## Features

### Dark Mode

- Automatic detection of system preference
- Manual toggle button in navbar
- Smooth transitions between modes
- Persistent preference storage

### ASCII Logo

- Monospace font rendering
- Responsive sizing for mobile
- Text shadow for visibility
- Purple color scheme

### Sidebar Navigation

- Left-side navigation (Docsy default)
- Active link highlighting
- Hover effects
- Mobile-responsive (collapsible on small screens)

### Responsive Design

- Mobile-first approach
- Breakpoints at 768px and 480px
- Touch-friendly buttons
- Optimized typography

## Customization

To modify colors, edit `assets/scss/_variables_project.scss`:

```scss
$primary: #7c3aed; // Change primary color
$secondary: #a78bfa; // Change secondary color
```

To modify dark mode colors, edit `assets/scss/_variables_project_after_bs.scss`.

To add custom styles, edit `assets/scss/_styles_project.scss`.

## Browser Support

- Modern browsers (Chrome, Firefox, Safari, Edge)
- CSS custom properties (CSS variables)
- Bootstrap 5 color modes
- Responsive design
