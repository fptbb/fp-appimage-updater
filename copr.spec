%global debug_package %{nil}

Name:           fp-appimage-updater
Version:        1.4.4
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

# Generate shell completions
mkdir -p ./temp_home
export HOME=$(pwd)/temp_home
export XDG_CONFIG_HOME=$(pwd)/temp_home/.config
export XDG_STATE_HOME=$(pwd)/temp_home/.local/state

./target/release/fp-appimage-updater completion bash > fp-appimage-updater.bash
./target/release/fp-appimage-updater completion zsh > _fp-appimage-updater
./target/release/fp-appimage-updater completion fish > fp-appimage-updater.fish

%install
mkdir -p %{buildroot}%{_bindir}
install -m 755 target/release/fp-appimage-updater %{buildroot}%{_bindir}/fp-appimage-updater

mkdir -p %{buildroot}%{_userunitdir}
install -m 644 systemd/fp-appimage-updater.service %{buildroot}%{_userunitdir}/
install -m 644 systemd/fp-appimage-updater.timer %{buildroot}%{_userunitdir}/

# Install shell completions
install -D -m 0644 fp-appimage-updater.bash %{buildroot}%{_datadir}/bash-completion/completions/%{name}
install -D -m 0644 _fp-appimage-updater %{buildroot}%{_datadir}/zsh/site-functions/_%{name}
install -D -m 0644 fp-appimage-updater.fish %{buildroot}%{_datadir}/fish/vendor_completions.d/%{name}.fish

%post
%systemd_user_post fp-appimage-updater.timer

%preun
%systemd_user_preun fp-appimage-updater.timer

%files
%{_bindir}/fp-appimage-updater
%{_userunitdir}/fp-appimage-updater.service
%{_userunitdir}/fp-appimage-updater.timer
%{_datadir}/bash-completion/completions/%{name}
%{_datadir}/zsh/site-functions/_%{name}
%{_datadir}/fish/vendor_completions.d/%{name}.fish
%doc README.md
%license LICENSE

%changelog
* Fri Apr 17 2026 fp-appimage-updater Maintainer - 1.4.4-2
- Add automatic shell completions for bash, zsh and fish
* Fri Apr 17 2026 fp-appimage-updater Maintainer - 1.4.4-1
- Update to version 1.4.4
* Fri Apr 17 2026 fp-appimage-updater Maintainer - 1.4.3-1
- Update to version 1.4.3
* Fri Apr 17 2026 fp-appimage-updater Maintainer - 1.4.2-1
- Update to version 1.4.2
* Fri Apr 17 2026 fp-appimage-updater Maintainer - 1.4.1-1
- Update to version 1.4.1
* Sun Apr 12 2026 fp-appimage-updater Maintainer - 1.4.0-1
- Update to version 1.4.0

