# Pull in the statically linked native half FIRST, so `OpenSSL` and its
# classes exist to be reopened below. This is CRuby's loader idiom -- see
# `gems/strscan/lib/strscan.rb` for the same shape and the reason for it.
require "openssl.so"

module OpenSSL
  # CRuby defines these in C (`ossl.c` and friends), but a feature-gated
  # native class cannot register a constructible exception in this runtime
  # (see `zeo-rt/src/ext/mod.rs`). Defined here they are ordinary user
  # classes -- registered under their fully qualified names with real
  # constructors -- which the native half raises by name.
  class OpenSSLError < StandardError; end

  class Digest
    class DigestError < OpenSSLError; end
  end

  class HMACError < OpenSSLError; end

  module KDF
    class KDFError < OpenSSLError; end
  end

  class BNError < OpenSSLError; end

  class BN
    include Comparable
  end

  module Random
    class RandomError < OpenSSLError; end
  end

  class Cipher
    class CipherError < OpenSSLError; end
    # A GCM/CCM/ChaCha20-Poly1305 tag that failed to verify -- its own class
    # so callers can tell tampering from a malformed stream.
    class AuthTagError < CipherError; end
  end

  module SSL
    class SSLError < OpenSSLError; end
    class SSLErrorWaitReadable < SSLError; end
    class SSLErrorWaitWritable < SSLError; end
  end

  module X509
    class StoreError < OpenSSLError; end
    class CertificateError < OpenSSLError; end
    class NameError < OpenSSLError; end
  end

  # OpenSSL::PKCS5 survives as a compatibility shim over OpenSSL::KDF, as in
  # CRuby's `openssl/pkcs5.rb`.
  module PKCS5
    module_function

    def pbkdf2_hmac(pass, salt, iter, keylen, digest)
      OpenSSL::KDF.pbkdf2_hmac(pass, salt: salt, iterations: iter,
                               length: keylen, hash: digest)
    end

    def pbkdf2_hmac_sha1(pass, salt, iter, keylen)
      pbkdf2_hmac(pass, salt, iter, keylen, "sha1")
    end
  end
end

# Double dispatch, as in CRuby's `openssl/bn.rb`.
class Integer
  # Casts an Integer as an OpenSSL::BN
  def to_bn
    OpenSSL::BN.new(self)
  end
end
