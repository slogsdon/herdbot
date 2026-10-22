#!/bin/sh

cargo install --path crates/herdbot
launchctl kickstart -k gui/$(id -u)/dev.herdbot.daemon
