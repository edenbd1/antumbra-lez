/****************************************************************************
** Meta object code from reading C++ file 'plugin.h'
**
** Created by: The Qt Meta Object Compiler version 69 (Qt 6.9.2)
**
** WARNING! All changes made in this file will be lost!
*****************************************************************************/

#include "../../../src/plugin.h"
#include <QtCore/qmetatype.h>
#include <QtCore/qplugin.h>

#include <QtCore/qtmochelpers.h>

#include <memory>


#include <QtCore/qxptype_traits.h>
#if !defined(Q_MOC_OUTPUT_REVISION)
#error "The header file 'plugin.h' doesn't include <QObject>."
#elif Q_MOC_OUTPUT_REVISION != 69
#error "This file was generated using the moc from 6.9.2. It"
#error "cannot be used with the include files from this version of Qt."
#error "(The moc has changed too much.)"
#endif

#ifndef Q_CONSTINIT
#define Q_CONSTINIT
#endif

QT_WARNING_PUSH
QT_WARNING_DISABLE_DEPRECATED
QT_WARNING_DISABLE_GCC("-Wuseless-cast")
namespace {
struct qt_meta_tag_ZN14AntumbraPluginE_t {};
} // unnamed namespace

template <> constexpr inline auto AntumbraPlugin::qt_create_metaobjectdata<qt_meta_tag_ZN14AntumbraPluginE_t>()
{
    namespace QMC = QtMocConstants;
    QtMocHelpers::StringRefStorage qt_stringData {
        "AntumbraPlugin"
    };

    QtMocHelpers::UintData qt_methods {
    };
    QtMocHelpers::UintData qt_properties {
    };
    QtMocHelpers::UintData qt_enums {
    };
    return QtMocHelpers::metaObjectData<AntumbraPlugin, qt_meta_tag_ZN14AntumbraPluginE_t>(QMC::MetaObjectFlag{}, qt_stringData,
            qt_methods, qt_properties, qt_enums);
}
Q_CONSTINIT const QMetaObject AntumbraPlugin::staticMetaObject = { {
    QMetaObject::SuperData::link<QObject::staticMetaObject>(),
    qt_staticMetaObjectStaticContent<qt_meta_tag_ZN14AntumbraPluginE_t>.stringdata,
    qt_staticMetaObjectStaticContent<qt_meta_tag_ZN14AntumbraPluginE_t>.data,
    qt_static_metacall,
    nullptr,
    qt_staticMetaObjectRelocatingContent<qt_meta_tag_ZN14AntumbraPluginE_t>.metaTypes,
    nullptr
} };

void AntumbraPlugin::qt_static_metacall(QObject *_o, QMetaObject::Call _c, int _id, void **_a)
{
    auto *_t = static_cast<AntumbraPlugin *>(_o);
    (void)_t;
    (void)_c;
    (void)_id;
    (void)_a;
}

const QMetaObject *AntumbraPlugin::metaObject() const
{
    return QObject::d_ptr->metaObject ? QObject::d_ptr->dynamicMetaObject() : &staticMetaObject;
}

void *AntumbraPlugin::qt_metacast(const char *_clname)
{
    if (!_clname) return nullptr;
    if (!strcmp(_clname, qt_staticMetaObjectStaticContent<qt_meta_tag_ZN14AntumbraPluginE_t>.strings))
        return static_cast<void*>(this);
    if (!strcmp(_clname, "IComponent"))
        return static_cast< IComponent*>(this);
    if (!strcmp(_clname, "com.logos.component.IComponent"))
        return static_cast< IComponent*>(this);
    return QObject::qt_metacast(_clname);
}

int AntumbraPlugin::qt_metacall(QMetaObject::Call _c, int _id, void **_a)
{
    _id = QObject::qt_metacall(_c, _id, _a);
    return _id;
}

#ifdef QT_MOC_EXPORT_PLUGIN_V2
static constexpr unsigned char qt_pluginMetaDataV2_AntumbraPlugin[] = {
    0xbf, 
    // "IID"
    0x02,  0x78,  0x1e,  'c',  'o',  'm',  '.',  'l', 
    'o',  'g',  'o',  's',  '.',  'c',  'o',  'm', 
    'p',  'o',  'n',  'e',  'n',  't',  '.',  'I', 
    'C',  'o',  'm',  'p',  'o',  'n',  'e',  'n', 
    't', 
    // "className"
    0x03,  0x6e,  'A',  'n',  't',  'u',  'm',  'b', 
    'r',  'a',  'P',  'l',  'u',  'g',  'i',  'n', 
    // "MetaData"
    0x04,  0xa9,  0x66,  'a',  'u',  't',  'h',  'o', 
    'r',  0x67,  'e',  'd',  'e',  'n',  'b',  'd', 
    '1',  0x65,  'b',  'u',  'i',  'l',  'd',  0xa2, 
    0x65,  'f',  'i',  'l',  'e',  's',  0x87,  0x6e, 
    's',  'r',  'c',  '/',  'p',  'l',  'u',  'g', 
    'i',  'n',  '.',  'c',  'p',  'p',  0x6c,  's', 
    'r',  'c',  '/',  'p',  'l',  'u',  'g',  'i', 
    'n',  '.',  'h',  0x74,  's',  'r',  'c',  '/', 
    'c',  'h',  'a',  'i',  'n',  '_',  'b',  'r', 
    'i',  'd',  'g',  'e',  '.',  'c',  'p',  'p', 
    0x72,  's',  'r',  'c',  '/',  'c',  'h',  'a', 
    'i',  'n',  '_',  'b',  'r',  'i',  'd',  'g', 
    'e',  '.',  'h',  0x6c,  'q',  'm',  'l',  '/', 
    'M',  'a',  'i',  'n',  '.',  'q',  'm',  'l', 
    0x6a,  'q',  'm',  'l',  '/',  'q',  'm',  'l', 
    'd',  'i',  'r',  0x71,  's',  'r',  'c',  '/', 
    'm',  'e',  't',  'a',  'd',  'a',  't',  'a', 
    '.',  'j',  's',  'o',  'n',  0x64,  't',  'y', 
    'p',  'e',  0x65,  'c',  'm',  'a',  'k',  'e', 
    0x68,  'c',  'a',  't',  'e',  'g',  'o',  'r', 
    'y',  0x64,  'd',  'e',  'f',  'i',  0x6c,  'd', 
    'e',  'p',  'e',  'n',  'd',  'e',  'n',  'c', 
    'i',  'e',  's',  0x80,  0x6b,  'd',  'e',  's', 
    'c',  'r',  'i',  'p',  't',  'i',  'o',  'n', 
    0x78,  0x98,  'A',  'n',  't',  'u',  'm',  'b', 
    'r',  'a',  ' ',  'l',  'a',  'u',  'n',  'c', 
    'h',  'p',  'a',  'd',  ' ',  'a',  'n',  'd', 
    ' ',  'v',  'e',  's',  't',  'i',  'n',  'g', 
    ' ',  uchar('\xe2'), uchar('\x80'), uchar('\x94'), ' ',  'r',  'e',  'a', 
    'd',  ' ',  't',  'h',  'e',  ' ',  'l',  'i', 
    'v',  'e',  ' ',  'o',  'n',  '-',  'c',  'h', 
    'a',  'i',  'n',  ' ',  's',  't',  'a',  't', 
    'e',  ' ',  'o',  'f',  ' ',  't',  'h',  'e', 
    ' ',  'b',  'o',  'n',  'd',  'i',  'n',  'g', 
    ' ',  'c',  'u',  'r',  'v',  'e',  ',',  ' ', 
    't',  'h',  'e',  ' ',  'L',  'B',  'P',  ' ', 
    'p',  'o',  'o',  'l',  ' ',  'a',  'n',  'd', 
    ' ',  'a',  ' ',  'v',  'e',  's',  't',  'i', 
    'n',  'g',  ' ',  's',  'c',  'h',  'e',  'd', 
    'u',  'l',  'e',  ',',  ' ',  's',  't',  'r', 
    'a',  'i',  'g',  'h',  't',  ' ',  'f',  'r', 
    'o',  'm',  ' ',  'a',  ' ',  'L',  'E',  'Z', 
    ' ',  's',  'e',  'q',  'u',  'e',  'n',  'c', 
    'e',  'r',  0x64,  'm',  'a',  'i',  'n',  0x6c, 
    'a',  'n',  't',  'u',  'm',  'b',  'r',  'a', 
    '_',  'l',  'e',  'z',  0x64,  'n',  'a',  'm', 
    'e',  0x6c,  'a',  'n',  't',  'u',  'm',  'b', 
    'r',  'a',  '_',  'l',  'e',  'z',  0x64,  't', 
    'y',  'p',  'e',  0x62,  'u',  'i',  0x67,  'v', 
    'e',  'r',  's',  'i',  'o',  'n',  0x65,  '0', 
    '.',  '1',  '.',  '0', 
    0xff, 
};
QT_MOC_EXPORT_PLUGIN_V2(AntumbraPlugin, AntumbraPlugin, qt_pluginMetaDataV2_AntumbraPlugin)
#else
QT_PLUGIN_METADATA_SECTION
Q_CONSTINIT static constexpr unsigned char qt_pluginMetaData_AntumbraPlugin[] = {
    'Q', 'T', 'M', 'E', 'T', 'A', 'D', 'A', 'T', 'A', ' ', '!',
    // metadata version, Qt version, architectural requirements
    0, QT_VERSION_MAJOR, QT_VERSION_MINOR, qPluginArchRequirements(),
    0xbf, 
    // "IID"
    0x02,  0x78,  0x1e,  'c',  'o',  'm',  '.',  'l', 
    'o',  'g',  'o',  's',  '.',  'c',  'o',  'm', 
    'p',  'o',  'n',  'e',  'n',  't',  '.',  'I', 
    'C',  'o',  'm',  'p',  'o',  'n',  'e',  'n', 
    't', 
    // "className"
    0x03,  0x6e,  'A',  'n',  't',  'u',  'm',  'b', 
    'r',  'a',  'P',  'l',  'u',  'g',  'i',  'n', 
    // "MetaData"
    0x04,  0xa9,  0x66,  'a',  'u',  't',  'h',  'o', 
    'r',  0x67,  'e',  'd',  'e',  'n',  'b',  'd', 
    '1',  0x65,  'b',  'u',  'i',  'l',  'd',  0xa2, 
    0x65,  'f',  'i',  'l',  'e',  's',  0x87,  0x6e, 
    's',  'r',  'c',  '/',  'p',  'l',  'u',  'g', 
    'i',  'n',  '.',  'c',  'p',  'p',  0x6c,  's', 
    'r',  'c',  '/',  'p',  'l',  'u',  'g',  'i', 
    'n',  '.',  'h',  0x74,  's',  'r',  'c',  '/', 
    'c',  'h',  'a',  'i',  'n',  '_',  'b',  'r', 
    'i',  'd',  'g',  'e',  '.',  'c',  'p',  'p', 
    0x72,  's',  'r',  'c',  '/',  'c',  'h',  'a', 
    'i',  'n',  '_',  'b',  'r',  'i',  'd',  'g', 
    'e',  '.',  'h',  0x6c,  'q',  'm',  'l',  '/', 
    'M',  'a',  'i',  'n',  '.',  'q',  'm',  'l', 
    0x6a,  'q',  'm',  'l',  '/',  'q',  'm',  'l', 
    'd',  'i',  'r',  0x71,  's',  'r',  'c',  '/', 
    'm',  'e',  't',  'a',  'd',  'a',  't',  'a', 
    '.',  'j',  's',  'o',  'n',  0x64,  't',  'y', 
    'p',  'e',  0x65,  'c',  'm',  'a',  'k',  'e', 
    0x68,  'c',  'a',  't',  'e',  'g',  'o',  'r', 
    'y',  0x64,  'd',  'e',  'f',  'i',  0x6c,  'd', 
    'e',  'p',  'e',  'n',  'd',  'e',  'n',  'c', 
    'i',  'e',  's',  0x80,  0x6b,  'd',  'e',  's', 
    'c',  'r',  'i',  'p',  't',  'i',  'o',  'n', 
    0x78,  0x98,  'A',  'n',  't',  'u',  'm',  'b', 
    'r',  'a',  ' ',  'l',  'a',  'u',  'n',  'c', 
    'h',  'p',  'a',  'd',  ' ',  'a',  'n',  'd', 
    ' ',  'v',  'e',  's',  't',  'i',  'n',  'g', 
    ' ',  uchar('\xe2'), uchar('\x80'), uchar('\x94'), ' ',  'r',  'e',  'a', 
    'd',  ' ',  't',  'h',  'e',  ' ',  'l',  'i', 
    'v',  'e',  ' ',  'o',  'n',  '-',  'c',  'h', 
    'a',  'i',  'n',  ' ',  's',  't',  'a',  't', 
    'e',  ' ',  'o',  'f',  ' ',  't',  'h',  'e', 
    ' ',  'b',  'o',  'n',  'd',  'i',  'n',  'g', 
    ' ',  'c',  'u',  'r',  'v',  'e',  ',',  ' ', 
    't',  'h',  'e',  ' ',  'L',  'B',  'P',  ' ', 
    'p',  'o',  'o',  'l',  ' ',  'a',  'n',  'd', 
    ' ',  'a',  ' ',  'v',  'e',  's',  't',  'i', 
    'n',  'g',  ' ',  's',  'c',  'h',  'e',  'd', 
    'u',  'l',  'e',  ',',  ' ',  's',  't',  'r', 
    'a',  'i',  'g',  'h',  't',  ' ',  'f',  'r', 
    'o',  'm',  ' ',  'a',  ' ',  'L',  'E',  'Z', 
    ' ',  's',  'e',  'q',  'u',  'e',  'n',  'c', 
    'e',  'r',  0x64,  'm',  'a',  'i',  'n',  0x6c, 
    'a',  'n',  't',  'u',  'm',  'b',  'r',  'a', 
    '_',  'l',  'e',  'z',  0x64,  'n',  'a',  'm', 
    'e',  0x6c,  'a',  'n',  't',  'u',  'm',  'b', 
    'r',  'a',  '_',  'l',  'e',  'z',  0x64,  't', 
    'y',  'p',  'e',  0x62,  'u',  'i',  0x67,  'v', 
    'e',  'r',  's',  'i',  'o',  'n',  0x65,  '0', 
    '.',  '1',  '.',  '0', 
    0xff, 
};
QT_MOC_EXPORT_PLUGIN(AntumbraPlugin, AntumbraPlugin)
#endif  // QT_MOC_EXPORT_PLUGIN_V2

QT_WARNING_POP
