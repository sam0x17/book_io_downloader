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

## CLI Syntax

```
A CLI utility for bulk-downloading book covers from book.io via the Cardano blockchain and IPFS

Usage: bcid [OPTIONS] --project-id <PROJECT_ID> <POLICY_ID> [OUTPUT_DIR]

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
