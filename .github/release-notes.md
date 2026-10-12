Apple Silicon（M1 以降）の Mac 用です。macOS 15 以降を対象にしていますが、**動作を確認したのは macOS 27.0.1（Apple Silicon）だけ**で、macOS 15〜26 では確認していません。

## インストール

1. `@DMG@` をダウンロードして開き、**Ambien** を **Applications** へドラッグします。
2. 初回だけ、Apple の署名・公証をしていないため、macOS が起動を止めます。次のどちらかで許可してください。
   - **システム設定 → プライバシーとセキュリティ** を開き、下の方に出る「"Ambien" はブロックされました」の **このまま開く** を押して、パスワードを入力します。
   - または、ターミナルで `xattr -dr com.apple.quarantine /Applications/Ambien.app` を実行してから開きます。
3. 2回目以降は普通に開けます。

## このリリースについて

- 先行配布（pre-release）です。**アプリ内の更新確認はありません**。新しい版は、Releases の新しい DMG を同じ手順で入れ直してください。設定は `~/Library/Application Support/Ambien/` に保存されるので、入れ直しても残ります。
- 署名は ad-hoc のみ（Developer ID・公証なし）です。DMG の SHA-256 は `@SHA256@` です（`shasum -a 256 @DMG@` で照合できます）。
- ソースは [takemo101/ambien](https://github.com/takemo101/ambien)、ライセンスは MIT です。同梱した第三者のライセンスは、アプリ内の `Contents/Resources/THIRD_PARTY_LICENSES.txt` にあります。
