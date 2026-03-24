Name:           fp-appimage-updater
Version:        1.1.4
Release:        1%{?dist}
Summary:        A lightweight declarative AppImage updater

License:        MIT
URL:            https://github.com/fptbb/fp-appimage-updater
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  systemd-rpm-macros
BuildRequires:  upx

%description
fp-appimage-updater is a lightweight, strictly declarative AppImage updater 
designed to manage Linux Desktop integration and AppImage binary updates natively.

%prep
%setup -q

%build
# We are building for release, but we must respect COPR's network access. 
# Cargo needs network access to fetch crates unless vendored. 
# Ensure "Enable network during build" is checked in COPR!
cargo build --release

# aggressively shrink the executable
upx --best --lzma target/release/fp-appimage-updater

%install
# Install the binary
mkdir -p %{buildroot}%{_bindir}
install -m 755 target/release/fp-appimage-updater %{buildroot}%{_bindir}/fp-appimage-updater

# Install systemd user units
mkdir -p %{buildroot}%{_userunitdir}
install -m 644 systemd/fp-appimage-updater.service %{buildroot}%{_userunitdir}/
install -m 644 systemd/fp-appimage-updater.timer %{buildroot}%{_userunitdir}/

%post
%systemd_user_post fp-appimage-updater.timer

%preun
%systemd_user_preun fp-appimage-updater.timer

%files
%{_bindir}/fp-appimage-updater
%{_userunitdir}/fp-appimage-updater.service
%{_userunitdir}/fp-appimage-updater.timer
# Assuming a standard README and LICENSE exist
%doc README.md
%license LICENSE

%changelog
* Mon Mar 23 2026 fp-appimage-updater Maintainer - 1.1.4-1
- Initial COPR release
