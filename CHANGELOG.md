# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.2] - 2026-02-02

### Version 0.7.2

#### Added
- Support for marking stateless resources as ready (#113)
- Links to k9s and Flux in the documentation (#111)

#### Changed
- Improved spacing and formatting in the documentation (#109, #111)
- Added demo videos to the documentation (#108)

#### Fixed
- Configuration option for the controller/operator namespace (#107)

## [0.7.1] - 2026-01-14

### Version 0.7.1

#### Changed
- Improved code readability and maintainability through code cleanup
- Updated Flux version display to show the current version

## [0.7.0] - 2026-01-13

### Version 0.7.0

#### Added
- Ability to open skin submenu from skin UI
- Controller pod status information

#### Changed
- Enhanced controller functionality

## [0.6.4] - 2026-01-09

### Version 0.6.4

#### Fixed
- Addressed version warning issues (#96)

## [0.6.3] - 2026-01-08

### Version 0.6.3

#### Added
- Added a sub-menu to the commands, accessible through the `ctx` command.

## [0.6.2] - 2026-01-06

### Version 0.6.2

#### Added
- Added backup fonts for documentation

#### Changed
- Refactored application structure, breaking up app struct (#91)

## [0.6.1] - 2025-12-30

### Version 0.6.1

#### Added
- Flux Webui features

#### Changed
- Version bumped to 0.6.0

## [0.6.0] - 2025-12-29

### Version 0.6.0

#### Added
- Added Flux WebUI features

## [0.5.11] - 2025-12-22

## Version 0.5.11

### Added
- Added "unhealthy" and "health" filters to display percentage status on top

### Changed
- Updated 2024 edition with status tweak

## [0.5.10] - 2025-12-22

Here is a concise changelog summary for version 0.5.10:

Added:
- Support for specifying a Kubernetes configuration file via the `--kubeconfig` option

## [0.5.9] - 2025-12-17

Here's a concise changelog summary for version 0.5.9:

### Fixed
- Resolved a bug that caused issues during application startup.

## [0.5.8] - 2025-12-12

## Version 0.5.8

### Added
- New `:ctx` for context switching
- Agent files for improved functionality

### Changed
- Website tweaks and improvements

## [0.5.7] - 2025-12-12

## Version 0.5.7

### Fixed
- Resolved issues with website and CI deployment

## [0.5.6] - 2025-12-12

### Version 0.5.6

#### Added
- Support for SOCKS proxy connections (#68)

#### Changed
- Compiled binary is statically linked

## [0.5.5] - 2025-12-07

## Version 0.5.5

### Changed
- Updated dependencies to address known security vulnerabilities

## [0.5.4] - 2025-12-07

### Version 0.5.4

#### Added
- Improved skin customization options
- Enhanced command-line interface (CLI)

#### Fixed
- Resolved scrolling bug reported in issue #61

## [0.5.3] - 2025-12-06

### Version 0.5.3

#### Added
- Ability to filter namespaces by number

#### Fixed
- Improvements to the help menu
- Various bug fixes and tweaks

## [0.5.2] - 2025-11-22

### Version 0.5.2

#### Changed
- Bumped version to 0.5.2

#### Fixed
- Resolved a CI issue

## [0.5.1] - 2025-11-22

### Version 0.5.1

#### Changed
- Refactored codebase and improved overall code hygiene (#51)

## [0.5.0] - 2025-11-22

## Version 0.5.0

### Added

- Better filtering
- Label and Annotation filtering
- Support for macOS 15 runners

## [0.4.3] - 2025-11-21

## Version 0.4.3

### Added

- Support for building on Windows platforms

## [0.4.2] - 2025-11-20

### Version 0.4.2

#### Fixed

- Resolved an issue with the wrap bug

#### Changed

- Refined the CI workflow configuration

## [0.4.1] - 2025-11-20

### Version 0.4.1

#### Changed

- Changed the CI agent model configuration (#40)
- Updated the CI workflow configuration (#38)

## [0.4.0] - 2025-11-20

### Added

- Support for Flux operator CRDs (#33)
- Screenshot image to documentation (#31, #32)

## [0.3.1] - 2025-11-20

### Fixed

- Fixed trace functionality (#28)
- Replaced hardcoded strings with configuration (#27)

## [0.3.0] - 2025-11-18

### Changed

- Default mode set to readOnly (#23)

## [0.2.4] - 2025-11-16

### Changed

- Workflow tweaks and improvements (#19)

## [0.2.2] - 2025-11-16

### Changed

- Code hygiene improvements (#16)
- Workflow release changes (#16)

## [0.2.1] - 2025-11-16

### Changed

- Version bump to 0.2.1

## [0.2.0] - 2025-11-16

### Added

- ReadOnly mode support
- Configuration and config-cli functionality

### Fixed

- OpenSSL build issues (#10, #8)
- Windows temporary file handling (#9)

## [0.1.5] - 2025-11-16

### Added

- OpenSSL support
- Debug logging (#7)
- Proxy support (#6)

## [0.1.4] - 2025-11-16

### Added

- Homebrew support (#5)

## [0.1.3] - 2025-11-16

### Added

- Homebrew macOS architecture support (#4)

## [0.1.2] - 2025-11-16

### Added

- Homebrew macOS architecture support (#3)
- macOS M-chip (Apple Silicon) support (#2)

## [0.1.1] - 2025-11-16

### Added

- Binstall support

## [0.1.0] - YYYY-MM-DD

### Added

- Initial release
- Real-time monitoring of Flux resources
- K9s-inspired terminal UI
- Support for all major Flux CRDs (Kustomization, GitRepository, HelmRelease, etc.)
- Resource operations (suspend, resume, reconcile, delete)
- YAML viewing
- Namespace switching
- Status indicators
- Comprehensive test suite

[Unreleased]: https://github.com/yourusername/flux9s/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/yourusername/flux9s/releases/tag/v0.1.0
