%global debug_package %{nil}

Name:           fp-appimage-updater
Version:        1.4.3
Release:        1%{?dist}
Summary:        A lightweight declarative AppImage updater

License:        MIT
URL:            https://gitlab.com/fpsys/fp-appimage-updater
Source0:        %{url}/-/archive/v%{version}/%{name}-v%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  systemd-rpm-macros
BuildRequires:  upx
BuildRequires:  openssl-devel
BuildRequires:  pkgconfig

%description
fp-appimage-updater is a lightweight, strictly declarative AppImage updater 
designed to manage Linux Desktop integration and AppImage binary updates natively.

%prep
%setup -q -n %{name}-v%{version}

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
* Fri Apr 17 2026 fp-appimage-updater Maintainer - 1.4.3-1
- Update to version 1.4.3
* Fri Apr 17 2026 fp-appimage-updater Maintainer - 1.4.2-1
- Update to version 1.4.2
* Fri Apr 17 2026 fp-appimage-updater Maintainer - 1.4.1-1
- Update to version 1.4.1
* Sun Apr 12 2026 fp-appimage-updater Maintainer - 1.4.0-1
- Update to version 1.4.0

