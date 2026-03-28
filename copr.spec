%global debug_package %{nil}

Name:           fp-appimage-updater
Version:        1.2.1
Release:        1%{?dist}
Summary:        A lightweight declarative AppImage updater

License:        MIT
URL:            https://github.com/fptbb/fp-appimage-updater
Source0:        %{url}/archive/refs/tags/v%{version}.tar.gz#/%{name}-%{version}.tar.gz

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
cargo build --release

upx --best --lzma target/release/fp-appimage-updater

%install
mkdir -p %{buildroot}%{_bindir}
install -m 755 target/release/fp-appimage-updater %{buildroot}%{_bindir}/fp-appimage-updater

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
%doc README.md
%license LICENSE

%changelog
* Sat Mar 28 2026 fp-appimage-updater Maintainer - 1.2.1-1
- Update to version 1.2.1
* Sat Mar 28 2026 fp-appimage-updater Maintainer - 1.2.0-1
- Update to version 1.2.0
* Fri Mar 27 2026 fp-appimage-updater Maintainer - 1.1.6-1
- Update to version 1.1.6
* Fri Mar 27 2026 fp-appimage-updater Maintainer - 1.1.5-2
- Update to version 1.1.5
* Tue Mar 27 2026 fp-appimage-updater Maintainer - 1.1.5-1
- Update to version 1.1.5
* Tue Mar 24 2026 fp-appimage-updater Maintainer - 1.1.4-1
- Initial COPR release

