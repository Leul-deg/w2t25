-- Districts
CREATE TABLE IF NOT EXISTS districts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    state TEXT,
    contact_email TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Campuses (a district can have multiple campuses)
CREATE TABLE IF NOT EXISTS campuses (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    district_id UUID NOT NULL REFERENCES districts(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    address TEXT,
    phone TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (district_id, name)
);

-- Schools (a campus can have multiple schools/programs)
CREATE TABLE IF NOT EXISTS schools (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    campus_id UUID NOT NULL REFERENCES campuses(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    school_type TEXT DEFAULT 'general',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (campus_id, name)
);

-- User-School assignments (teachers/staff assigned to schools)
CREATE TABLE IF NOT EXISTS user_school_assignments (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    school_id UUID NOT NULL REFERENCES schools(id) ON DELETE CASCADE,
    assignment_type TEXT NOT NULL DEFAULT 'staff',
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, school_id)
);

-- Classes / Homerooms
CREATE TABLE IF NOT EXISTS classes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    school_id UUID NOT NULL REFERENCES schools(id) ON DELETE CASCADE,
    teacher_id UUID REFERENCES users(id),
    name TEXT NOT NULL,
    grade_level TEXT,
    academic_year TEXT NOT NULL DEFAULT TO_CHAR(NOW(), 'YYYY'),
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Class enrollments (students enrolled in classes)
CREATE TABLE IF NOT EXISTS class_enrollments (
    class_id UUID NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
    student_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    enrolled_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'withdrawn', 'transferred')),
    PRIMARY KEY (class_id, student_id)
);

-- Parent-Student relationships
CREATE TABLE IF NOT EXISTS parent_student_links (
    parent_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    student_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    relationship TEXT NOT NULL DEFAULT 'parent',
    PRIMARY KEY (parent_id, student_id)
);

CREATE INDEX IF NOT EXISTS idx_campuses_district_id ON campuses(district_id);
CREATE INDEX IF NOT EXISTS idx_schools_campus_id ON schools(campus_id);
CREATE INDEX IF NOT EXISTS idx_classes_school_id ON classes(school_id);
CREATE INDEX IF NOT EXISTS idx_classes_teacher_id ON classes(teacher_id);
CREATE INDEX IF NOT EXISTS idx_class_enrollments_student_id ON class_enrollments(student_id);
CREATE INDEX IF NOT EXISTS idx_parent_student_links_parent_id ON parent_student_links(parent_id);
CREATE INDEX IF NOT EXISTS idx_parent_student_links_student_id ON parent_student_links(student_id);
