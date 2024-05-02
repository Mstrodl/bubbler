#!/bin/bash

set -euo pipefail
TARGET=armv7-unknown-linux-gnueabihf
cross build --target=$TARGET --release
for f in bepis bigdrink littledrink snack; do
	ssh root@$f rm '~root/bubbler'
	scp target/$TARGET/release/bubbler root@$f:~/bubbler
	ssh root@$f systemctl restart bubbler
	echo "+ $f"
done
echo "Should be deployed?"
