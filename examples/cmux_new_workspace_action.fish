#!/usr/bin/env fish
cmux new-workspace --focus true --cwd $WTS_DIR --name "(wts) "(basename $WTS_DIR) && echo $WTS_DIR | pbcopy
