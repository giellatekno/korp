#!/usr/bin/env bash

echo "executing cqp..."
counts=$(cqp -e -r compiled_mono/cwb/registry/ -f count_words_in_corpora.cqpscript | cut -f1)

sum=0
while IFS= read -r line; do
    sum=$(($sum + $line))
done <<< "$counts"

echo "$sum"
