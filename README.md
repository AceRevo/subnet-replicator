# SubNet Replicator

A portable, incremental command-line backup tool for IDE and development workspaces, designed to run from USB storage drives.

## Overview

SubNet Replicator is a standalone utility written in Rust. It automatically scans the host system for development project folders (such as IntelliJ IDEA directories) and incrementally synchronizes them to the storage drive.

## Features

- **Portable:** Single executable with no external dependencies or runtime requirements.
- **Incremental:** Compares file sizes and modification timestamps to transfer only modified or new files.
- **Terminal UI:** Fast terminal output with ANSI colors.
- **Non-destructive:** Does not delete files on the target drive.

## Usage

Place the executable on your USB drive and run it from the drive:

```bash
SUBNET_REPLICATOR.exe
```

Press Enter to confirm and begin synchronization.

## License

MIT License
