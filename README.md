# Book.io Cover Image Downloader (BCID)

This crate contains a CLI utility for downloading cover images for books stored in the Cardano
blockchain via Books.io and a client library that provides a public interface for this
functionality allowing it to be used independently of the CLI.

The public API is fully async and all downloads are done in parallel. Terminal-based progress
bars are provided that can optionally be turned off using the `--quiet` option or by setting
`client.quiet` to true when using the library. If existing partial or complete downloads of the
same cover images that are being downloaded are detected in the target directory, they will be
resumed or skipped accordingly, making this process fully idempotent when run on the same
`policy_id`, assuming the ordering of assets returned by Cardano does not change.

## Installation / Requirements

### BlockFrost Project ID

A valid BlockFrost API key / project id is required to use the CLI application and/or the
client library. Although the option exists to specify this manually via the `--project-id`
option, _you should not do this_ unless you know what you are doing. Instead you should store
your project id in an environment variable called `BLOCKFROST_PROJECT_ID`. The client library
is also configured to check this environment variable when initialized via the `Client::new()`
method.

As a general rule of thumb, your BlockFrost project id should never be committed to version
control or stored in a place where others could easily obtain or see it.

### IPFS Daemon

To use the CLI application or the client library, you _must_ have an IPFS daemon running in the
background so we can download covers from IPFS. If running on linux, the popular `kubo` package
usually fills this requirement.

On Arch Linux, this is enough to get you up and running:

```$
$ sudo pacman -S kubo
$ ipfs init
$ ipfs daemon
```

## CLI Syntax

```
A CLI utility for bulk-downloading book covers from book.io via the Cardano blockchain and IPFS

Usage: bcid [OPTIONS] <POLICY_ID> [OUTPUT_DIR]

Arguments:
  <POLICY_ID>
          Specifies the Cardano `policy_id`/`collection_id` you would like to download cover images for

  [OUTPUT_DIR]
          Specifies the filesystem directory you would like to download cover images to.
          
          Defaults to the current directory if not specified.
          
          If partial or complete downloads of any of the cover images about to be downloaded already exist in the specified directory, they will be resumed or skipped accordingly.

Options:
      --project-id <PROJECT_ID>
          Specifies the BlockFrost API `project_id` that will be used to query Cardano. This is required and automatically loaded from the `BLOCKFROST_PROJECT_ID` env var
          
          [env: BLOCKFROST_PROJECT_ID=REDACTED]

  -n, --num-covers <NUM_COVERS>
          Specifies the number of cover images that should be downloaded for the specified `policy_id`. Defaults to 10

  -a, --api-url <API_URL>
          Overrides the default book.io valid covers API URL with the specified URL.
          
          Must be a GET endpoint that conforms to the JSON schema utilized by https://api.book.io/api/v0/collections, which is the default value.

  -q, --quiet
          If enabled, suppresses all progress bar terminal output

  -s, --slow
          If enabled, simulates a slow network connection using manual sleeps for easier debugging and visual inspection of download progress bars

      --simulate-kill
          If enabled, will kill each download at a random percent completion (progress bars will suggest 100% completion).
          
          This is useful for debugging/testing the idempotence of the CLI when followed up by a subsequent call without `--simulate-kill`.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```
## Library Example

This demonstrates basic usage of the client library to download 5 cover images for the book
"The Wizard Tim". This will display terminal progress bars for each download because we don't
set `client.quiet` to true.

```rust
const THE_WIZARD_TIM_POLICY_ID: &'static str =
    "c40ca49ac9fe48b86d6fd998645b5c8ac89a4e21e2cfdb9fdca3e7ac";

// must be declared mutable so we can call `download_covers_for_policy`
// because valid collection ids are cached on the client
let mut client = Client::new().unwrap(); // blockfrost project ID automatically read from ENV


let results: Vec<(String, PathBuf)> = client
    .download_covers_for_policy(THE_WIZARD_TIM_POLICY_ID, 5, PathBuf::from("/some/dir"))
    .await
    .unwrap();

assert_eq!(res.len(), 5);
for (cid, path) in res {
    assert_eq!(cid.len(), 53);
    let metadata = std::fs::metadata(path).unwrap();
    assert_eq!(metadata.is_file(), true);
    assert_ne!(metadata.len(), 0);
}
```

For more information see the docs for `Client` and related types.
