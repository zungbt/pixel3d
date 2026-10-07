# Quy ước làm việc

## 1. Branch

- `main` luôn chạy được. **Không commit thẳng lên `main`.**
- Mỗi feature/fix một nhánh, tách từ `main` mới nhất:

  | Prefix | Dùng khi |
  |---|---|
  | `feat/<tên>` | Thêm tính năng (`feat/zoom-out`, `feat/dirt-path`) |
  | `fix/<tên>` | Sửa lỗi (`fix/grass-under-rocks`) |
  | `refactor/<tên>` | Sắp xếp lại code, không đổi hành vi |
  | `docs/<tên>` | Chỉ sửa tài liệu |

  Tên nhánh: chữ thường, nối bằng `-`.

```bash
git switch main && git pull
git switch -c feat/<tên>
```

## 2. Commit

- Mỗi commit một ý, message tiếng Anh, dòng đầu ngắn gọn mô tả *cái gì đổi*:
  - `Zoom out: VIEW_HEIGHT 13 -> 18`
  - `fix: vColor is vec4 in three r186`
- Trước khi commit: `npm run build` phải qua, và mở `npm run dev` xem lại cảnh bằng mắt (thay đổi hình ảnh không có test tự động).

## 3. Merge vào `main`

Merge bằng `--no-ff` để lịch sử giữ lại từng nhánh feature:

```bash
git switch main && git pull
git merge --no-ff feat/<tên> -m "merge feat/<tên>: <tóm tắt>"
git push origin main
git branch -d feat/<tên>
```

## 4. Version (SemVer)

Version có dạng `MAJOR.MINOR.PATCH`, được ghi ở **hai nơi luôn khớp nhau**: `version` trong `package.json` và git tag `vX.Y.Z`.

| Tăng | Khi nào | Ví dụ |
|---|---|---|
| `PATCH` | Sửa lỗi, chỉnh nhỏ (màu, thông số) | `0.1.0` → `0.1.1` |
| `MINOR` | Thêm feature mới | `0.1.1` → `0.2.0` |
| `MAJOR` | Thay đổi lớn / phá vỡ cách dùng cũ; `1.0.0` = bản chính thức đầu tiên | `0.9.0` → `1.0.0` |

Không phải merge nào cũng ra version — gom vài feature rồi release một lần cũng được.

## 5. Release

Trên `main`, sau khi đã merge xong:

```bash
git switch main && git pull
npm version minor -m "release v%s"   # hoặc patch / major
git push origin main --tags
```

`npm version` tự sửa `package.json` + `package-lock.json`, tạo commit `release vX.Y.Z` và tag `vX.Y.Z`.

## 6. Tra cứu version

```bash
git tag -n                          # danh sách version
git switch --detach v0.1.0          # xem lại một bản cũ (quay về: git switch main)
git diff v0.1.0 v0.2.0              # so sánh hai bản
git log --merges --oneline v0.1.0..v0.2.0   # các feature/fix có trong một release
```
