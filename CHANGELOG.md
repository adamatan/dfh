# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.3](https://github.com/adamatan/dfh/compare/v0.1.2...v0.1.3) - 2026-04-18

### Bug Fixes

- let cargo-dist own GitHub Release creation

## [0.1.0](https://github.com/adamatan/dfh/releases/tag/v0.1.0) - 2026-04-12

### Bug Fixes

- allow unnecessary_cast for f_frsize (needed on 32-bit Linux)
- gate container_disk to macOS, fix u64 cast lint on Linux

### Features

- human-readable disk usage with colorized bars and system volume annotations
