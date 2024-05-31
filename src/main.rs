use anyhow::{Context, Result};
use std::os::unix::fs;
use tempfile::tempdir;

// Usage: your_docker.sh run <image> <command> <arg1> <arg2> ...
fn main() -> Result<()> {
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    // eprintln!("Logs from your program will appear here!");

    // Uncomment this block to pass the first stage!
    let args: Vec<_> = std::env::args().collect();
    let command = &args[3];
    let command_args = &args[4..];

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
