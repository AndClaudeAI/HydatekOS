/*
 * A laptop's DSDT in miniature, written the way firmware vendors write them:
 * an Intel LPSS I2C controller on PCI with a Synaptics-style HID over I2C
 * touchpad (enabled from _INI through _OSI and a NVS field), an AMD-style
 * I2C controller described by memory, a battery, a light sensor, and the
 * idioms their methods use (Switch, ToUUID, ConcatenateResTemplate,
 * CreateDWordField patching, Index/DerefOf, While).
 */
DefinitionBlock ("", "DSDT", 2, "HYDTK", "LAPTOP", 1)
{
    Name (OSYS, 0x07D0)
    OperationRegion (GNVS, SystemMemory, 0x1000, 0x20)
    Field (GNVS, AnyAcc, Lock, Preserve)
    {
        TPTY, 8,
        BCAP, 16,
        BREM, 16,
        , 4,
        FLG4, 4,
        ALSV, 32
    }

    Scope (\_SB)
    {
        Method (_INI, 0, NotSerialized)
        {
            If (_OSI ("Windows 2009")) { OSYS = 0x07D9 }
            If (_OSI ("Windows 2015")) { OSYS = 0x07DF }
            If (_OSI ("Linux")) { OSYS = 0x03E8 }
        }

        Device (PCI0)
        {
            Name (_HID, EisaId ("PNP0A08"))
            Name (_CID, EisaId ("PNP0A03"))
            Name (_BBN, Zero)

            Device (I2C1)
            {
                Name (_ADR, 0x00150001)
                Method (FMCN, 0, NotSerialized)
                {
                    Name (PKG, Package (0x03) { 0x0101, 0x012C, 0x62 })
                    Return (PKG)
                }
                OperationRegion (ICFG, PCI_Config, Zero, 0x100)
                Field (ICFG, DWordAcc, NoLock, Preserve)
                {
                    VDID, 32
                }
                Method (VEND, 0, NotSerialized)
                {
                    Return (VDID & 0xFFFF)
                }
            }
        }

        Device (I2CA)
        {
            Name (_HID, "AMDI0010")
            Name (_UID, Zero)
            Name (RBUF, ResourceTemplate ()
            {
                Memory32Fixed (ReadWrite, 0x00000000, 0x00001000, _Y00)
                Interrupt (ResourceConsumer, Edge, ActiveHigh, Exclusive, ,, ) { 0x0000000A }
            })
            Method (_CRS, 0, NotSerialized)
            {
                CreateDWordField (RBUF, \_SB.I2CA._Y00._BAS, BADR)
                BADR = 0xFEDC2000
                Return (RBUF)
            }
            Method (_STA, 0, NotSerialized) { Return (0x0F) }
        }

        Device (BAT0)
        {
            Name (_HID, EisaId ("PNP0C0A"))
            Name (_UID, One)
            Method (_BIF, 0, NotSerialized)
            {
                Name (BPKG, Package (0x0D)
                {
                    One, 0x1770, 0x1388, One, 0x2B5C, Zero, Zero, 0x40, 0x40, "BAT", "123", "LION", "Hydatek"
                })
                BPKG [0x02] = BCAP
                Return (BPKG)
            }
            Method (_BST, 0, NotSerialized)
            {
                Local0 = Package (0x04) { One, 0x01F4, Zero, 0x2EE0 }
                Local0 [0x02] = BREM
                Return (Local0)
            }
        }

        Device (ALS0)
        {
            Name (_HID, "ACPI0008")
            Method (_ALI, 0, NotSerialized) { Return (ALSV) }
        }

        Device (LID0)
        {
            Name (_HID, EisaId ("PNP0C0D"))
            Method (_STA, 0, NotSerialized) { Return (Zero) }
        }

        Method (SUMN, 1, Serialized)
        {
            Local0 = Zero
            Local1 = Zero
            While (Local1 < Arg0)
            {
                Local1++
                If (Local1 == 0x03) { Continue }
                Local0 += Local1
                If (Local1 >= 0x0A) { Break }
            }
            Return (Local0)
        }

        Method (PKGT, 0, Serialized)
        {
            Name (TBL, Package (0x03) { 0x10, "two", Buffer (0x02) { 0xAA, 0xBB } })
            Local0 = DerefOf (Index (TBL, 0x02))
            Local1 = SizeOf (TBL)
            Local2 = DerefOf (Local0 [One])
            Return ((Local1 << 0x08) | Local2)
        }

        Method (STRS, 0, NotSerialized)
        {
            Local0 = Concatenate ("Hyda", "tek")
            If (Local0 == "Hydatek") { Return (ToInteger ("0x2A")) }
            Return (Zero)
        }

        Method (MTCH, 0, NotSerialized)
        {
            Return (Match (Package () { 5, 10, 15, 20 }, MGT, 0x0B, MTR, Zero, Zero))
        }
    }

    Scope (\_SB.PCI0.I2C1)
    {
        Device (TPD0)
        {
            Name (_HID, "SYNA2393")
            Name (_CID, "PNP0C50")
            Name (_UID, One)
            Name (HID2, Zero)
            Name (SBFB, ResourceTemplate ()
            {
                I2cSerialBusV2 (0x002C, ControllerInitiated, 0x00061A80,
                    AddressingMode7Bit, "\\_SB.PCI0.I2C1",
                    0x00, ResourceConsumer, , Exclusive, )
            })
            Name (SBFG, ResourceTemplate ()
            {
                GpioInt (Level, ActiveLow, ExclusiveAndWake, PullDefault, 0x0000,
                    "\\_SB.GPI0", 0x00, ResourceConsumer, , )
                    { 0x0055 }
            })
            Method (_INI, 0, NotSerialized)
            {
                HID2 = 0x20
            }
            Method (_STA, 0, NotSerialized)
            {
                If (OSYS >= 0x07DC)
                {
                    If (TPTY == One) { Return (0x0F) }
                }
                Return (Zero)
            }
            Method (_CRS, 0, NotSerialized)
            {
                Return (ConcatenateResTemplate (SBFB, SBFG))
            }
            Method (_DSM, 4, Serialized)
            {
                If (Arg0 == ToUUID ("3cdff6f7-4267-4555-ad05-b30a3d8938de"))
                {
                    Switch (ToInteger (Arg2))
                    {
                        Case (Zero)
                        {
                            Switch (ToInteger (Arg1))
                            {
                                Case (One) { Return (Buffer (One) { 0x03 }) }
                                Default { Return (Buffer (One) { 0x00 }) }
                            }
                        }
                        Case (One) { Return (HID2) }
                    }
                }
                Return (Buffer (One) { 0x00 })
            }
        }

        Device (TPL1)
        {
            Name (_HID, "ELAN2514")
            Name (_CID, "PNP0C50")
            Method (_STA, 0, NotSerialized) { Return (Zero) }
        }
    }
}
