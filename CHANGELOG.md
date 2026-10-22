# Changelog

## 1.1.0

### New
- **About screen**: click the ⓘ button in the top bar for version info and a
  link to the project
- **Update check**: on launch the app asks GitHub whether a newer release
  exists. If one does, a dot appears on the ⓘ button and the About screen
  links to the download page. Nothing is installed automatically
- **Light theme**: the app now follows your system's light or dark mode

### Improved
- Uses your OS's own title bar and window buttons, so moving, resizing,
  snapping and minimizing work the way you expect on each platform
- Much lower CPU use: scanning slows down once the window has been still for
  a few seconds (corners dim and the status reads "Idle"), and slows further
  once a QR code is found. Moving or hovering over the window resumes full
  speed
- Smoother window dragging: scanning pauses while the window is moving
- Easier-to-read text, with better contrast in both themes
- Rounded viewfinder corners and a more noticeable flash when a code is found

### Fixed
- Symbols that showed up as empty boxes on some systems
