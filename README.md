# sigma-scribe-plugins

The only source of plugins for [Sigma Scribe](https://github.com/armandomartires/sigma-scribe):
fonts, dictionaries, themes, layouts, translations, HWR models and (later) icon packs.
The app downloads from here and nowhere else.

## Layout

```
plugins/<id>/plugin.conf     manifest
plugins/<id>/...             data files, named as the app expects
index.json                   generated: what the app browses (signed)
```

A plugin's files use the same relative paths the app already reads from
`.adds/sigma-scribe/`, for example `fonts/cjk.ttf`, `dict/skk-jisyo.txt`,
`dict/pinyin-hans.txt`.

## plugin.conf

Plain `key=value` lines, unknown keys ignored.

```
id=cjk-font            # lowercase letters, digits, '-'
name=CJK font
version=1.0.0
kind=font              # font|dictionary|hwr-model|word-list|theme|translation|layout|icons
description=Noto Sans CJK for Chinese, Japanese and Korean
min_app=1.11.0
api=1
```

Plugins are data only; no code runs. Each plugin keeps its own licence
notice inside its folder.
