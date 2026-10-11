#!/bin/bash -eu

find . -type f -name '*.py' -exec ruff format {} +
find . -type f -name '*.py' -exec ruff check --select I --fix {} +
find . -type f -name '*.py' -exec ruff check --fix {} +
