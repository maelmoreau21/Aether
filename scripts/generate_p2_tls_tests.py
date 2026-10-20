"""Create disposable local certificate fixtures; requires Python cryptography."""
import datetime
from pathlib import Path
from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.hazmat.primitives.serialization import pkcs12

out = Path(__file__).resolve().parents[1] / "target/p2-tls-tests"
out.mkdir(parents=True, exist_ok=True)
now = datetime.datetime.now(datetime.timezone.utc)


def authority(label):
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    name = x509.Name([x509.NameAttribute(x509.NameOID.COMMON_NAME, label)])
    cert = (x509.CertificateBuilder().subject_name(name).issuer_name(name).public_key(key.public_key())
        .serial_number(x509.random_serial_number()).not_valid_before(now - datetime.timedelta(days=3))
        .not_valid_after(now + datetime.timedelta(days=1)).add_extension(x509.BasicConstraints(ca=True, path_length=None), critical=True)
        .sign(key, hashes.SHA256()))
    return key, cert


key, root = authority("Disposable P2 test root")
(out / "ca.pem").write_bytes(root.public_bytes(serialization.Encoding.PEM))
for label in ["valid", "expired", "untrusted"]:
    issuer_key, issuer = authority("Untrusted P2 test root") if label == "untrusted" else (key, root)
    leaf_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    leaf = (x509.CertificateBuilder().subject_name(x509.Name([x509.NameAttribute(x509.NameOID.COMMON_NAME, "localhost")]))
        .issuer_name(issuer.subject).public_key(leaf_key.public_key()).serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(days=2)).not_valid_after(now + datetime.timedelta(days=-1 if label == "expired" else 1))
        .add_extension(x509.SubjectAlternativeName([x509.DNSName("localhost")]), critical=False)
        .sign(issuer_key, hashes.SHA256()))
    (out / (label + ".p12")).write_bytes(pkcs12.serialize_key_and_certificates(b"test", leaf_key, leaf, [issuer], serialization.NoEncryption()))
print(out)
