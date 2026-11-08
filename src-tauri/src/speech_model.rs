//! The Whisper speech model. It's too big to ship inside the installer, so
//! it's downloaded once on first launch into the app's data folder.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, Runtime};
use tokio::{fs, io::AsyncWriteExt};

/// Multilingual Whisper "base", quantized: a third of the full model's size
/// and faster on CPU, with nearly the same accuracy.
const FILE_NAME: &str = "ggml-base-q5_1.bin";
const URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin";
const SIZE: u64 = 59_707_625;
const SHA256: &str = "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898";

/// Returns the model's path, downloading it first if it isn't there yet.
/// `on_progress` gets `(bytes downloaded, total bytes)`.
pub async fn ensure<R: Runtime>(
    app: &AppHandle<R>,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Couldn't find the app's data folder: {e}"))?
        .join("models");
    let path = dir.join(FILE_NAME);
    if fs::metadata(&path)
        .await
        .is_ok_and(|meta| meta.len() == SIZE)
    {
        return Ok(path);
    }

    fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let partial = path.with_extension("part");
    download(&partial, &mut on_progress).await?;
    fs::rename(&partial, &path)
        .await
        .map_err(|e| format!("Couldn't save the speech model: {e}"))?;
    Ok(path)
}

async fn download(to: &Path, on_progress: &mut impl FnMut(u64, u64)) -> Result<(), String> {
    let mut response = reqwest::get(URL)
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| download_error(&e))?;
    let mut file = fs::File::create(to).await.map_err(|e| save_error(&e))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0;
    on_progress(0, SIZE);

    while let Some(chunk) = response.chunk().await.map_err(|e| download_error(&e))? {
        file.write_all(&chunk).await.map_err(|e| save_error(&e))?;
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;
        on_progress(downloaded, SIZE);
    }
    file.flush().await.map_err(|e| save_error(&e))?;

    let hash: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if downloaded != SIZE || hash != SHA256 {
        let _ = fs::remove_file(to).await;
        return Err(
            "The speech model download was incomplete or corrupted. Press Try again.".into(),
        );
    }
    Ok(())
}

fn download_error(error: &reqwest::Error) -> String {
    if error.is_connect() || error.is_timeout() || error.is_body() {
        "Couldn't download the speech model. Check your internet connection, then press Try again."
            .into()
    } else {
        format!("Couldn't download the speech model ({error}). Press Try again.")
    }
}

fn save_error(error: &std::io::Error) -> String {
    format!(
        "Couldn't save the speech model ({error}). It needs about 60 MB of free disk space. Press Try again."
    )
}
