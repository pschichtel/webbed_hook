# Webbed Hook

This program a rather simple program that
implement's [git-receive-pack's hook protocol](https://git-scm.com/docs/git-receive-pack) and translates the invocation
into an HTTP request to a URL that is configurable per repository.

The main purpose of this software is to implement validation hooks (primarily pre-receive hook) by delegating the
decision over the web to another application. It is intended to be installed globally (as in: installed in every
repository on the server) on a git server, allowing hook configuration per repository using a JSON file.

It's primarily tested as a GitLab global server hook, and it does process GitLab's `GL_*` environment variables, but it
does _not_ depend on GitLab.

Supported hooks:

* `pre-receive`
* `update`
* `post-receive`

The `post-update` hook is intentionally not available as it has been superseded by the `post-receive`, which provides
the same functionality but additional information.

## Installation

Installation is generally very simple: Just place the binary in the `.git/hooks` folder of the repository named either
`pre-receive`, `update` or `post-receive`. The process is no different from for any other hook installation. The binary
can also be symlinked, which might be desired when installed it for multiple hooks.

### GitLab

The installation when using GitLab is slightly different and is generally covered
by [this documentation page](https://docs.gitlab.com/ee/administration/server_hooks.html). GitLab supports per-project
hooks as well as global hooks and this project also supports both, but global hooks are the primary focus.

Global hooks must be enabled first by configuring a `custom_hooks_dir` in gitaly's `hooks` section either directly in
the `gitaly/config.toml` or through your `gitlab.rb` configuration (when using an omnibus package).

Inside the configured directory you have two options:

1. Place the binary directly in there similar to the `.git/hooks` for git repositories.
2. Create `<hook>.d` subfolders, so `pre-receive.d`, `update.d` and/or `post-receive.d`, and place the binary in any of
   these folders with an arbitrary name.

Once again, both options are supported by this project.

## Hook Configuration

No matter how the hook is installed, by default no action is performed and the process terminated very quickly without
side effects.

In order to activate hooks for a repository the repository's default branch must contain a file called `hooks.json`.
The file follows the schema defined in [`config.schema.json`](config.schema.json), so please check that and/or configure
your text editor to use it for completion and validation.

On the top-level sections exist for each supported hook with the same name and each section has the same options.

More details are available in the following example and in the schema definition.

### Example

```json5
// $schema: config.schema.json
{
  // Currently it must always be `1`.
  "version": "1",
  // Probably the most important hook, but the sections for `update` and `post-receive` are identical
  "pre-receive": {
    // This section configures which refs should be subject to the hook.
    // At least once selector must exist. 
    "ref-selectors": [
      {
        "type": "branch",
        "name": "main"
      },
      {
        "type": "tag",
        "name": "v1.0.0"
      },
      {
        "type": "ref-regex",
        // Limitations apply, see: https://github.com/rust-lang/regex
        "pattern": "^refs/heads/.+$"
      }
    ],
    // The target url for the webhook. See the following section for details.
    "url": "https://example.org/webhook",
    // This option takes arbitrary JSON and passes it on to the webhook.
    "config": {
      "some key": "some value"
    },
    // Whether to reject the hook when anything regarding the webhook request failed.
    "reject-on-error": true,
    "request-timeout": 1000,
    "connect-timeout": 1000,
    // Optional messages always printed to the client before performing the webhook.
    "greeting-messages": [
      "Hi there!"
    ],
    // Whether to include a patch file (git-format-patch) for the changes.
    "include-patch": true,
    // Whether to include a commit log (git-log) for the changes.
    "include-log": true,
    // allow bypassing this specific hook by a push option and optionally print messages to the client 
    "bypass": {
      "push-option": "some_option_name",
      "messages": [
        "Hooks bypassed by global option"
      ]
    }
  },
  // allow bypassing any of the configured hooks by a push option and optionally print messages to the client 
  "bypass": {
    "push-option": "some_option_name",
    "messages": [
      "Hooks bypassed by global option"
    ]
  }
}
```

## Webhook Receivers

The webhook receiver application can be any HTTP/1.1-capable web server, that can accept a request. The request will
carry a body of type `application/json` including all information about the hook invocation and its context. Its schema
is described in [`request.schema.json`](request.schema.json). The receiver can respond with any HTTP status code in the
200-299 range to accept the hook execution and with any other status code to reject it.

Optionally the response can have a body of type `application/json` in order to provide information about why the hook
was accepted or rejected. Its schema is described in [`response.schema.json`](response.schema.json). Accept messages
will be printed to stdout, rejection messages will be printed to stderr.