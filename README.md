# Webbed Hook

This program a rather simple program that implement's [git-receive-pack's hook protocol](https://git-scm.com/docs/git-receive-pack) and translates the invocation into a HTTP request to an URL that is configurable per repository.

The main purpose of this software is to implement validation hooks (primarily pre-receive hook) by delegating the decision over the web to another application. It is intended to be installed globally (as in: installed in every repository on the server) on a git server, allowing hook configuration per repository using a JSON file.

It's primarily tested as a GitLab global server hook and it does process gitlab's `GL_*` environment variables, but it does _not_ depend on GitLab.

## Installation

TODO

## Hook Configuration

TODO

## Webhook Receivers
