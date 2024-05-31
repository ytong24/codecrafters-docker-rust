use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::{os::unix::fs, path::Path};
use tempfile::tempdir;

const DOCKER_HUB_LIB_URL: &str = "https://registry.hub.docker.com/v2/library";

fn get_auth_token(image: &str) -> Result<String> {
    let url = format!(
        "https://auth.docker.io/token?service=registry.docker.io&scope=repository:library/{}:pull",
        image
    );
    let body = reqwest::blocking::get(&url)?.json::<serde_json::Value>()?;
    let token = String::from(body["token"].as_str().unwrap());
    Ok(token)
}

fn get_blob_shasums(image: &str, tag: &str, token: &str) -> Result<Vec<String>> {
    let url = format!("{}/{}/manifests/{}", DOCKER_HUB_LIB_URL, image, tag);
    let mut shasums = vec![];

    // Get manifests
    let client = reqwest::blocking::Client::new();
    let body = client
        .get(&url)
        .bearer_auth(token)
        .send()?
        .json::<serde_json::Value>()?;

    // parse manifests and get shasums
    if let Some(fs_layers) = body["fsLayers"].as_array() {
        for elem in fs_layers {
            shasums.push(String::from(elem["blobSum"].as_str().unwrap()));
        }
    }

    Ok(shasums)
}

fn write_blobs(
    image: &str,
    blob_shasums: &Vec<String>,
    token: &str,
    root_dir: &Path,
) -> Result<()> {
    let client = reqwest::blocking::Client::new();

    for shasum in blob_shasums {
        let url = format!("{}/{}/blobs/{}", DOCKER_HUB_LIB_URL, image, shasum);
        let blob = client.get(&url).bearer_auth(token).send()?.bytes()?;

        let tar = GzDecoder::new(&blob[..]);
        let mut archive = tar::Archive::new(tar);
        archive.set_preserve_permissions(true);
        archive.set_unpack_xattrs(true);
        archive.unpack(root_dir)?;
    }

    Ok(())
}

fn pull_image(image: &str, tag: &str, root_dir: &Path) -> Result<()> {
    let token = get_auth_token(image)?;

    let blob_shasums = get_blob_shasums(image, tag, &token)?;
    write_blobs(image, &blob_shasums, &token, root_dir)?;

    Ok(())
}

// Usage: your_docker.sh run <image> <command> <arg1> <arg2> ...
fn main() -> Result<()> {
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    // eprintln!("Logs from your program will appear here!");

    // Uncomment this block to pass the first stage!
    let args: Vec<_> = std::env::args().collect();
    let image_tag = &args[2];
    let command = &args[3];
    let command_args = &args[4..];

    let (image, tag) = match image_tag.split_once(":") {
        Some((image, tag)) => (image, tag),
        None => (image_tag.as_str(), "latest"),
    };

    // Create a tmp_dir as the root dir of the new process
    let tmp_root = tempdir()?;

    // Copy the local exec file to the tmp_dir
    let to = tmp_root
        .path()
        .join(command.strip_prefix("/").unwrap_or(&command));
    std::fs::create_dir_all(to.parent().unwrap())?;
    std::fs::copy(command, to)?;

    // Create the /dev/null under the new root
    let dev_null = tmp_root.path().join("dev/null");
    std::fs::create_dir_all(dev_null.parent().unwrap())?;
    std::fs::File::create(dev_null)?;

    // Reconstruct each layer in the image under the new root dir
    pull_image(image, tag, tmp_root.path())?;

    // Change the root dir of current process, so that the child process will use the new root dir
    fs::chroot(tmp_root.path())?;

    unsafe { libc::unshare(libc::CLONE_NEWPID) };

    let output = std::process::Command::new(command)
        .args(command_args)
        .output()
        .with_context(|| {
            format!(
                "Tried to run '{}' with arguments {:?}",
                command, command_args
            )
        })?;

    let std_out = std::str::from_utf8(&output.stdout)?;
    print!("{}", std_out);
    let std_err = std::str::from_utf8(&output.stderr)?;
    eprint!("{}", std_err);

    let exit_code = output.status.code().unwrap_or(1);
    std::process::exit(exit_code);

    // Ok(())
}
