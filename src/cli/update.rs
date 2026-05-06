use anyhow::{bail, Context, Result};
use colored::Colorize;

const REPO: &str = "johangm90/spex";
const BINARY: &str = "spex";

/// Archive format for the current platform.
#[allow(dead_code)]
enum ArchiveFormat {
    TarGz,
    Zip,
}

/// Detect the current platform target triple and archive format.
fn current_platform() -> Result<(&'static str, ArchiveFormat)> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Ok(("aarch64-apple-darwin", ArchiveFormat::TarGz));

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return Ok(("x86_64-apple-darwin", ArchiveFormat::TarGz));

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Ok(("x86_64-unknown-linux-gnu", ArchiveFormat::TarGz));

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return Ok(("aarch64-unknown-linux-gnu", ArchiveFormat::TarGz));

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Ok(("x86_64-pc-windows-msvc", ArchiveFormat::Zip));

    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    return Ok(("aarch64-pc-windows-msvc", ArchiveFormat::Zip));

    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64"),
    )))]
    bail!(
        "unsupported platform — please download manually from https://github.com/{REPO}/releases"
    );
}

/// Fetch the latest release tag from the GitHub API.
async fn fetch_latest_version(client: &reqwest::Client) -> Result<String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = client
        .get(&url)
        .header("User-Agent", format!("spex/{}", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("failed to reach GitHub API")?;

    if !resp.status().is_success() {
        bail!("GitHub API returned {}", resp.status());
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse GitHub API response")?;
    let tag = json["tag_name"]
        .as_str()
        .context("missing tag_name in GitHub API response")?
        .to_string();
    Ok(tag)
}

/// Download bytes from a URL.
async fn download_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let resp = client
        .get(url)
        .header("User-Agent", format!("spex/{}", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .with_context(|| format!("failed to download {url}"))?;

    if !resp.status().is_success() {
        bail!("download failed with status {}", resp.status());
    }

    let bytes = resp.bytes().await.context("failed to read response body")?;
    Ok(bytes.to_vec())
}

/// Extract the `spex` / `spex.exe` binary from a tar.gz archive.
fn extract_from_targz(data: Vec<u8>) -> Result<Vec<u8>> {
    let gz = flate2::read::GzDecoder::new(std::io::Cursor::new(data));
    let mut archive = tar::Archive::new(gz);

    for entry in archive.entries().context("failed to read tar archive")? {
        let mut entry = entry.context("failed to read tar entry")?;
        let path = entry.path().context("invalid tar entry path")?;
        if path.ends_with(BINARY) {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut buf)
                .context("failed to read binary from archive")?;
            return Ok(buf);
        }
    }

    bail!("binary '{BINARY}' not found inside tar.gz archive");
}

/// Extract the `spex.exe` binary from a zip archive.
fn extract_from_zip(data: Vec<u8>) -> Result<Vec<u8>> {
    use std::io::Read;

    let cursor = std::io::Cursor::new(data);
    let mut zip = zip::ZipArchive::new(cursor).context("failed to open zip archive")?;

    let exe_name = format!("{BINARY}.exe");
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).context("failed to read zip entry")?;
        let name = file.name().to_string();
        // Match <archive_name>/spex.exe or just spex.exe
        if name == exe_name || name.ends_with(&format!("/{exe_name}")) {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .context("failed to read binary from zip")?;
            return Ok(buf);
        }
    }

    bail!("binary '{exe_name}' not found inside zip archive");
}

/// Replace the running executable with `new_bytes`.
///
/// On Unix this is a simple write + atomic rename.
/// On Windows the running exe is locked, so we rename it to `.old` first,
/// write the new binary, then schedule the old file for deletion on next boot
/// via `MoveFileExW(MOVEFILE_DELAY_UNTIL_REBOOT)`.
fn replace_binary(exe_path: &std::path::Path, new_bytes: &[u8]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let tmp_path = exe_path.with_extension("tmp");
        std::fs::write(&tmp_path, new_bytes).context("failed to write temporary binary")?;
        let mut perms = std::fs::metadata(&tmp_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&tmp_path, perms)?;
        std::fs::rename(&tmp_path, exe_path).context(
            "failed to replace binary — try running with sudo or check file permissions",
        )?;
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        // Rename the running exe out of the way (Windows allows renaming open files)
        let old_path = exe_path.with_extension("old");
        std::fs::rename(exe_path, &old_path)
            .context("failed to rename current binary — check file permissions")?;

        // Write the new binary
        std::fs::write(exe_path, new_bytes).context("failed to write new binary")?;

        // Schedule the .old file for deletion on next reboot
        let old_wide: Vec<u16> = old_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: calling a well-documented Win32 API with valid null-terminated wide strings.
        unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                old_wide.as_ptr(),
                std::ptr::null(),
                windows_sys::Win32::Storage::FileSystem::MOVEFILE_DELAY_UNTIL_REBOOT,
            );
        }
    }

    Ok(())
}

pub async fn cmd_update(check_only: bool) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    let client = reqwest::Client::new();

    print!("Checking for updates… ");
    let latest_tag = fetch_latest_version(&client).await?;
    // Strip leading 'v' for comparison
    let latest_version = latest_tag.trim_start_matches('v');

    if latest_version == current_version {
        println!("{}", "already up to date.".green());
        println!("  version: {}", current_version.bold());
        return Ok(());
    }

    println!(
        "{} → {}",
        current_version.yellow(),
        latest_version.green().bold()
    );

    if check_only {
        println!("\nRun {} to install the update.", "spex update".bold());
        return Ok(());
    }

    let (target, fmt) = current_platform()?;

    let (archive_ext, archive_name) = match fmt {
        ArchiveFormat::TarGz => ("tar.gz", format!("{BINARY}-{latest_tag}-{target}")),
        ArchiveFormat::Zip => ("zip", format!("{BINARY}-{latest_tag}-{target}")),
    };

    let archive_url = format!(
        "https://github.com/{REPO}/releases/download/{latest_tag}/{archive_name}.{archive_ext}"
    );

    println!("Downloading {}.{}…", archive_name.bold(), archive_ext);
    let archive_bytes = download_bytes(&client, &archive_url).await?;

    let binary_bytes = match fmt {
        ArchiveFormat::TarGz => extract_from_targz(archive_bytes)?,
        ArchiveFormat::Zip => extract_from_zip(archive_bytes)?,
    };

    let exe_path =
        std::env::current_exe().context("failed to determine current executable path")?;

    replace_binary(&exe_path, &binary_bytes)?;

    println!(
        "{} Updated to {} ✓",
        "✔".green().bold(),
        latest_version.green().bold()
    );

    Ok(())
}
