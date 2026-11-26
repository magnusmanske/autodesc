#!/usr/bin/env bash
# Delopy and restart toolforge services
ssh login.toolforge.org 'become autodesc bash -lc "cd ~/autodesc/ && git pull && ./build.sh && ./restart.sh && echo \"OK\""'
