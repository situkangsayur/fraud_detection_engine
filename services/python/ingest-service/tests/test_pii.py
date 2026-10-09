from app.inference.pii import detect_pii, is_email, is_pan, is_phone, luhn_ok, mask


def test_luhn_and_pan() -> None:
    assert luhn_ok("4111111111111111")
    assert not luhn_ok("4111111111111112")
    assert is_pan("4111 1111 1111 1111")
    assert is_pan("3782-822463-10005")
    assert not is_pan("081234567890")  # phone: not Luhn/length
    assert not is_pan("1234567890123")


def test_phone_and_email() -> None:
    for p in ("081234567890", "+6281298765432", "6285711112222", "0812-3456-7890"):
        assert is_phone(p), p
    assert not is_phone("12345")
    assert is_email("a.b+c@example.co.id")
    assert not is_email("not-an-email")


def test_detect_pii_kinds() -> None:
    assert detect_pii("no_kartu", ["4111 1111 1111 1111", "5500 0000 0000 0004"]) == "pan"
    assert detect_pii("user.email", ["a@x.com", "b@y.id"]) == "email"
    assert detect_pii("no_hp", ["081234567890", "082233334444"]) == "phone"
    assert detect_pii("rekening_tujuan", ["1234567890", "9876543210"]) == "account_number"
    assert detect_pii("nama_pelanggan", ["Budi Santoso"]) == "name"
    assert detect_pii("nominal", ["1000", "2000"]) is None


def test_masking() -> None:
    assert mask("4111 1111 1111 1111", "pan") == "411111******1111"
    assert mask("budi@example.com", "email") == "b***@example.com"
    assert mask("Budi Santoso", "name") == "B*** S***"
    assert "****" in mask("081234567890", "phone")
